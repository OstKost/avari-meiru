package http

import (
	"context"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"

	"mieru-admin/internal/domain"
	"mieru-admin/internal/service"
)

type mockMitaCtrl struct {
	installed     bool
	version       string
	systemdActive bool
	proxyRunning  bool
}

func (m *mockMitaCtrl) IsInstalled() bool                               { return m.installed }
func (m *mockMitaCtrl) GetVersion() (string, error)                     { return m.version, nil }
func (m *mockMitaCtrl) IsSystemdActive() bool                           { return m.systemdActive }
func (m *mockMitaCtrl) GetMitaStatus() (bool, bool, error)              { return true, m.proxyRunning, nil }
func (m *mockMitaCtrl) ReplaceConfig(string) error                      { return nil }
func (m *mockMitaCtrl) Start() error                                    { m.proxyRunning = true; return nil }
func (m *mockMitaCtrl) Stop() error                                     { m.proxyRunning = false; return nil }
func (m *mockMitaCtrl) Reload() error                                   { return nil }
func (m *mockMitaCtrl) Restart() error                                  { m.proxyRunning = true; return nil }
func (m *mockMitaCtrl) StartSystemd() error                             { m.systemdActive = true; return nil }
func (m *mockMitaCtrl) StopSystemd() error                              { m.systemdActive = false; return nil }

func setupTestServer(t *testing.T) (*Server, *Client, string) {
	tmpDir := t.TempDir()
	confPath := filepath.Join(tmpDir, "server_config.json")
	initial := `{
    "portBindings": [{"port": 28443, "protocol": "TCP"}],
    "users": [{"name": "admin@test", "hashedPassword": "abcd"}],
    "loggingLevel": "INFO",
    "mtu": 1400
}`
	_ = os.WriteFile(confPath, []byte(initial), 0600)

	cfgMgr := domain.NewConfigManager(confPath)
	ctrl := &mockMitaCtrl{installed: true, version: "3.38.0", systemdActive: true, proxyRunning: true}
	installer := service.NewInstallerService(ctrl)
	logsSvc := &service.LogsService{}
	adminSvc := service.NewAdminService(cfgMgr, ctrl, installer, logsSvc, "1.0.0", "test")

	token := "test-secret-token-1234567890abcdef"
	srv := NewServer(ServerConfig{
		Addr:       "127.0.0.1:0",
		TokenValue: token,
	}, adminSvc)

	// Use httptest.Server for actual network calls
	ts := httptest.NewServer(srv.loggingAndLimitMiddleware(srv.authMiddleware(srv.mux)))
	t.Cleanup(ts.Close)

	client := NewClient(ts.URL+"/v1", token)
	return srv, client, token
}

func TestAuthMiddleware(t *testing.T) {
	_, client, _ := setupTestServer(t)
	ctx := context.Background()

	// 1. Success with valid token
	status, err := client.GetStatus(ctx)
	if err != nil {
		t.Fatalf("expected success with valid token: %v", err)
	}
	if !status.Mita.Installed {
		t.Errorf("expected mita installed")
	}

	// 2. Failure with invalid token
	badClient := NewClient(client.BaseURL, "invalid-token")
	_, err = badClient.GetStatus(ctx)
	if err == nil {
		t.Fatalf("expected error with invalid token, got nil")
	}

	// 3. Failure with empty token
	noTokenClient := NewClient(client.BaseURL, "")
	_, err = noTokenClient.GetStatus(ctx)
	if err == nil {
		t.Fatalf("expected error with empty token, got nil")
	}
}

func TestHTTP_ConfigAndConflict(t *testing.T) {
	_, client, _ := setupTestServer(t)
	ctx := context.Background()

	// GET config
	cfg, err := client.GetConfig(ctx)
	if err != nil {
		t.Fatalf("GetConfig: %v", err)
	}
	if cfg.Revision == "" {
		t.Fatalf("expected non-empty revision")
	}

	// Preview config
	preview, err := client.PreviewConfig(ctx, map[string]any{
		"portBindings": []any{map[string]any{"port": float64(30000), "protocol": "TCP"}},
	})
	if err != nil {
		t.Fatalf("PreviewConfig: %v", err)
	}
	if !preview.Valid || preview.Diff == "" {
		t.Errorf("preview invalid or empty diff: %+v", preview)
	}

	// Apply config with correct revision
	candMap := map[string]any{
		"portBindings": []any{map[string]any{"port": float64(30000), "protocol": "TCP"}},
		"users":        []any{map[string]any{"name": "admin@test", "hashedPassword": "abcd"}},
		"loggingLevel": "INFO",
		"mtu":          float64(1400),
	}
	applyRes, err := client.ApplyConfig(ctx, cfg.Revision, candMap)
	if err != nil {
		t.Fatalf("ApplyConfig: %v", err)
	}
	if !applyRes.Success || applyRes.Revision == "" {
		t.Errorf("unexpected apply result: %+v", applyRes)
	}

	// Conflict: apply again with stale revision
	_, err = client.ApplyConfig(ctx, cfg.Revision, candMap)
	if err == nil {
		t.Fatalf("expected conflict 409 error with stale revision, got nil")
	}
}

func TestHTTP_UserLifecycleAndExport(t *testing.T) {
	_, client, _ := setupTestServer(t)
	ctx := context.Background()

	// Add user
	rev, err := client.AddUser(ctx, "newuser@mieru.io", "StrongSecretPass!")
	if err != nil {
		t.Fatalf("AddUser: %v", err)
	}
	if rev == "" {
		t.Errorf("expected non-empty revision")
	}

	// List users
	users, err := client.ListUsers(ctx)
	if err != nil {
		t.Fatalf("ListUsers: %v", err)
	}
	if len(users) != 2 {
		t.Errorf("expected 2 users, got %d", len(users))
	}

	// Export profile with correct password
	profile, err := client.ExportProfile(ctx, "newuser@mieru.io", "StrongSecretPass!", "31.58.213.192", 28443)
	if err != nil {
		t.Fatalf("ExportProfile failed: %v", err)
	}
	if profile.Username != "newuser@mieru.io" || profile.Password != "StrongSecretPass!" || profile.Server != "31.58.213.192" {
		t.Errorf("unexpected profile: %+v", profile)
	}

	// Export profile with wrong password must fail with 401
	_, err = client.ExportProfile(ctx, "newuser@mieru.io", "WrongPassword", "31.58.213.192", 28443)
	if err == nil {
		t.Fatalf("expected export to fail with wrong password, got nil")
	}

	// Reset password
	_, err = client.ResetUserPassword(ctx, "newuser@mieru.io", "UpdatedPass999")
	if err != nil {
		t.Fatalf("ResetUserPassword: %v", err)
	}

	// Export with updated pass
	_, err = client.ExportProfile(ctx, "newuser@mieru.io", "UpdatedPass999", "31.58.213.192", 28443)
	if err != nil {
		t.Fatalf("ExportProfile with updated pass failed: %v", err)
	}

	// Delete user
	_, err = client.DeleteUser(ctx, "newuser@mieru.io")
	if err != nil {
		t.Fatalf("DeleteUser: %v", err)
	}

	usersAfter, _ := client.ListUsers(ctx)
	if len(usersAfter) != 1 {
		t.Errorf("expected 1 user left, got %d", len(usersAfter))
	}
}

func TestHTTP_ServiceControlAndLogs(t *testing.T) {
	_, client, _ := setupTestServer(t)
	ctx := context.Background()

	// Control service
	res, err := client.ControlService(ctx, "stop")
	if err != nil {
		t.Fatalf("ControlService stop: %v", err)
	}
	if res.MitaRunning {
		t.Errorf("expected mita_running false")
	}

	res, err = client.ControlService(ctx, "start")
	if err != nil {
		t.Fatalf("ControlService start: %v", err)
	}
	if !res.MitaRunning {
		t.Errorf("expected mita_running true")
	}

	// Logs
	logs, err := client.GetLogs(ctx, 50)
	if err != nil {
		t.Fatalf("GetLogs: %v", err)
	}
	if len(logs) == 0 {
		t.Errorf("expected at least log message")
	}
}
