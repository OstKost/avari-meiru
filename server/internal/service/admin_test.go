package service

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"mieru-admin/internal/domain"
)

type MockMitaController struct {
	installed       bool
	version         string
	systemdActive   bool
	daemonRunning   bool
	proxyRunning    bool
	failReplace     bool
	failReload      bool
	lastReplacedCfg string
}

func (m *MockMitaController) IsInstalled() bool                               { return m.installed }
func (m *MockMitaController) GetVersion() (string, error)                     { return m.version, nil }
func (m *MockMitaController) IsSystemdActive() bool                           { return m.systemdActive }
func (m *MockMitaController) GetMitaStatus() (bool, bool, error)              { return m.daemonRunning, m.proxyRunning, nil }
func (m *MockMitaController) ReplaceConfig(p string) error {
	if m.failReplace {
		return os.ErrPermission
	}
	data, _ := os.ReadFile(p)
	m.lastReplacedCfg = string(data)
	return nil
}
func (m *MockMitaController) Start() error {
	m.proxyRunning = true
	return nil
}
func (m *MockMitaController) Stop() error {
	m.proxyRunning = false
	return nil
}
func (m *MockMitaController) Reload() error {
	if m.failReload {
		return os.ErrInvalid
	}
	return nil
}
func (m *MockMitaController) Restart() error {
	m.proxyRunning = true
	return nil
}
func (m *MockMitaController) StartSystemd() error {
	m.systemdActive = true
	m.daemonRunning = true
	m.proxyRunning = true
	return nil
}
func (m *MockMitaController) StopSystemd() error {
	m.systemdActive = false
	m.daemonRunning = false
	m.proxyRunning = false
	return nil
}

func setupTestAdmin(t *testing.T) (*AdminService, *MockMitaController, string) {
	tmpDir := t.TempDir()
	confPath := filepath.Join(tmpDir, "server_config.json")
	initial := `{
    "portBindings": [{"port": 28443, "protocol": "TCP"}],
    "users": [{"name": "root@local", "hashedPassword": "dummy", "allowPrivateIP": false}],
    "loggingLevel": "INFO",
    "mtu": 1400
}`
	_ = os.WriteFile(confPath, []byte(initial), 0600)

	cfgMgr := domain.NewConfigManager(confPath)
	mockCtrl := &MockMitaController{
		installed:     true,
		version:       "3.38.0",
		systemdActive: true,
		daemonRunning: true,
		proxyRunning:  true,
	}
	installer := NewInstallerService(mockCtrl)
	logsSvc := &LogsService{}

	svc := NewAdminService(cfgMgr, mockCtrl, installer, logsSvc, "1.0.0", "test")
	svc.configPath = confPath
	return svc, mockCtrl, confPath
}

func TestAdminService_GetStatusAndConfig(t *testing.T) {
	svc, _, _ := setupTestAdmin(t)
	ctx := context.Background()

	status, err := svc.GetStatus(ctx)
	if err != nil {
		t.Fatalf("GetStatus: %v", err)
	}
	if !status.Mita.Installed || status.Mita.Version != "3.38.0" || !status.Mita.ProxyRunning {
		t.Errorf("unexpected status: %+v", status.Mita)
	}
	if status.Mita.Port != 28443 || status.Mita.UsersCount != 1 {
		t.Errorf("unexpected port or users count: port=%d, count=%d", status.Mita.Port, status.Mita.UsersCount)
	}

	cfg, err := svc.GetConfig(ctx)
	if err != nil {
		t.Fatalf("GetConfig: %v", err)
	}
	if cfg.Revision == "" || len(cfg.Users) != 1 || cfg.Users[0].Name != "root@local" {
		t.Errorf("unexpected config result: %+v", cfg)
	}
}

func TestAdminService_UserLifecycle(t *testing.T) {
	svc, mockCtrl, _ := setupTestAdmin(t)
	ctx := context.Background()

	// 1. Add user
	rev, err := svc.AddUser(ctx, "user1@domain.com", "Password_12345", nil, nil)
	if err != nil {
		t.Fatalf("AddUser: %v", err)
	}
	if rev == "" {
		t.Errorf("expected new revision")
	}
	if mockCtrl.lastReplacedCfg == "" {
		t.Errorf("expected mita replace config to be invoked")
	}

	// 2. Export profile
	profile, err := svc.ExportProfile(ctx, "user1@domain.com", "Password_12345", "10.0.0.1", 28443)
	if err != nil {
		t.Fatalf("ExportProfile: %v", err)
	}
	if profile.Username != "user1@domain.com" || profile.Server != "10.0.0.1" || profile.ServerPort != 28443 {
		t.Errorf("unexpected exported profile: %+v", profile)
	}

	// 3. Reset password
	rev2, err := svc.ResetUserPassword(ctx, "user1@domain.com", "NewPass_98765")
	if err != nil {
		t.Fatalf("ResetUserPassword: %v", err)
	}
	if rev2 == rev {
		t.Errorf("expected revision to change after reset")
	}

	// Old pass should fail export
	_, err = svc.ExportProfile(ctx, "user1@domain.com", "Password_12345", "10.0.0.1", 28443)
	if err == nil {
		t.Fatalf("expected old password to fail export")
	}

	// New pass succeeds
	_, err = svc.ExportProfile(ctx, "user1@domain.com", "NewPass_98765", "10.0.0.1", 28443)
	if err != nil {
		t.Fatalf("expected new password to succeed export: %v", err)
	}

	// 4. Delete user
	_, err = svc.DeleteUser(ctx, "user1@domain.com")
	if err != nil {
		t.Fatalf("DeleteUser: %v", err)
	}

	users, _ := svc.ListUsers(ctx)
	if len(users) != 1 {
		t.Errorf("expected 1 user left, got %d", len(users))
	}
}

func TestAdminService_ApplyRollbackOnFailure(t *testing.T) {
	svc, mockCtrl, confPath := setupTestAdmin(t)
	ctx := context.Background()

	// Initial content
	origBytes, _ := os.ReadFile(confPath)

	cfg, _ := svc.GetConfig(ctx)
	baseRev := cfg.Revision

	// Make mock controller fail on replace
	mockCtrl.failReplace = true

	candMap := map[string]any{
		"portBindings": []any{map[string]any{"port": float64(55555), "protocol": "TCP"}},
		"users":        []any{},
		"loggingLevel": "INFO",
		"mtu":          float64(1400),
	}

	_, err := svc.ApplyConfig(ctx, baseRev, candMap)
	if err == nil {
		t.Fatalf("expected apply to fail when mita replace fails")
	}

	// Verify file was restored to original content
	afterBytes, _ := os.ReadFile(confPath)
	if string(afterBytes) != string(origBytes) {
		t.Errorf("expected config file to be rolled back to original content!\nGot: %s\nWant: %s", string(afterBytes), string(origBytes))
	}
}

func TestAdminService_PreviewConfig(t *testing.T) {
	svc, _, _ := setupTestAdmin(t)
	ctx := context.Background()

	patch := map[string]any{
		"portBindings": []any{map[string]any{"port": float64(30000), "protocol": "TCP"}},
	}

	preview, err := svc.PreviewConfig(ctx, patch)
	if err != nil {
		t.Fatalf("PreviewConfig: %v", err)
	}
	if !preview.Valid {
		t.Errorf("expected preview to be valid, err: %s", preview.ValidationError)
	}
	if preview.Diff == "" {
		t.Errorf("expected diff to be non-empty")
	}
}

func TestAdminService_ServiceControl(t *testing.T) {
	svc, mockCtrl, _ := setupTestAdmin(t)
	ctx := context.Background()

	res, err := svc.ControlService(ctx, "stop")
	if err != nil || mockCtrl.proxyRunning {
		t.Fatalf("stop service failed: %v", err)
	}
	if res.MitaRunning {
		t.Errorf("expected mita_running false")
	}

	res, err = svc.ControlService(ctx, "start")
	if err != nil || !mockCtrl.proxyRunning {
		t.Fatalf("start service failed: %v", err)
	}
	if !res.MitaRunning {
		t.Errorf("expected mita_running true")
	}
}
