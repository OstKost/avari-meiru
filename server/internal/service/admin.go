package service

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"

	"mieru-admin/internal/domain"
)

type AdminService struct {
	mu           sync.Mutex
	cfgManager   *domain.ConfigManager
	mitaCtrl     MitaController
	installer    *InstallerService
	logsSvc      *LogsService
	version      string
	commit       string
	startTime    time.Time
	configPath   string
}

func NewAdminService(
	cfgManager *domain.ConfigManager,
	mitaCtrl MitaController,
	installer *InstallerService,
	logsSvc *LogsService,
	version, commit string,
) *AdminService {
	if version == "" {
		version = "1.0.0"
	}
	if commit == "" {
		commit = "dev"
	}
	cfgPath := "/etc/mita/server_config.json"
	if cfgManager != nil && cfgManager.ConfigPath() != "" {
		cfgPath = cfgManager.ConfigPath()
	}
	return &AdminService{
		cfgManager: cfgManager,
		mitaCtrl:   mitaCtrl,
		installer:  installer,
		logsSvc:    logsSvc,
		version:    version,
		commit:     commit,
		startTime:  time.Now(),
		configPath: cfgPath,
	}
}

// StatusResult maps to OpenAPI StatusResponse schema
type StatusResult struct {
	OS struct {
		Name     string `json:"name"`
		Version  string `json:"version"`
		Arch     string `json:"arch"`
		Systemd  bool   `json:"systemd"`
		PublicIP string `json:"public_ip,omitempty"`
	} `json:"os"`
	Mita struct {
		Installed       bool   `json:"installed"`
		Version         string `json:"version,omitempty"`
		ExpectedVersion string `json:"expected_version"`
		SystemdActive   bool   `json:"systemd_active"`
		DaemonRunning   bool   `json:"daemon_running"`
		ProxyRunning    bool   `json:"proxy_running"`
		Port            int    `json:"port"`
		UsersCount      int    `json:"users_count"`
	} `json:"mita"`
	MieruAdmin struct {
		Version       string `json:"version"`
		Commit        string `json:"commit"`
		UptimeSeconds int64  `json:"uptime_seconds"`
	} `json:"mieru_admin"`
}

// DetectPublicOrLocalIP finds the best candidate external or local IP without external third-party calls
func DetectPublicOrLocalIP() string {
	addrs, err := net.InterfaceAddrs()
	if err != nil {
		return "127.0.0.1"
	}
	for _, a := range addrs {
		if ipnet, ok := a.(*net.IPNet); ok && !ipnet.IP.IsLoopback() {
			if ipnet.IP.To4() != nil {
				ip := ipnet.IP.String()
				// Return non-loopback IPv4
				return ip
			}
		}
	}
	return "127.0.0.1"
}

func (s *AdminService) GetStatus(ctx context.Context) (*StatusResult, error) {
	res := &StatusResult{}
	res.OS.Arch = runtime.GOARCH
	res.OS.Name = runtime.GOOS
	res.OS.PublicIP = DetectPublicOrLocalIP()

	// Detect distro from /etc/os-release if Linux
	if data, err := os.ReadFile("/etc/os-release"); err == nil {
		lines := strings.Split(string(data), "\n")
		for _, l := range lines {
			if strings.HasPrefix(l, "PRETTY_NAME=") {
				res.OS.Name = strings.Trim(strings.TrimPrefix(l, "PRETTY_NAME="), `"`)
			} else if strings.HasPrefix(l, "VERSION_ID=") {
				res.OS.Version = strings.Trim(strings.TrimPrefix(l, "VERSION_ID="), `"`)
			}
		}
	}
	if _, err := os.Stat("/run/systemd/system"); err == nil {
		res.OS.Systemd = true
	}

	res.Mita.ExpectedVersion = ExpectedMitaVersion
	res.Mita.Installed = s.mitaCtrl.IsInstalled()
	if res.Mita.Installed {
		ver, err := s.mitaCtrl.GetVersion()
		if err == nil {
			res.Mita.Version = ver
		}
	}
	res.Mita.SystemdActive = s.mitaCtrl.IsSystemdActive()
	daemonRun, proxyRun, _ := s.mitaCtrl.GetMitaStatus()
	res.Mita.DaemonRunning = daemonRun
	res.Mita.ProxyRunning = proxyRun

	rc, err := s.cfgManager.LoadConfig()
	if err == nil {
		res.Mita.Port = rc.GetPrimaryTCPPort()
		res.Mita.UsersCount = len(rc.ListUsers())
	} else {
		res.Mita.Port = 28443
		res.Mita.UsersCount = 0
	}

	res.MieruAdmin.Version = s.version
	res.MieruAdmin.Commit = s.commit
	res.MieruAdmin.UptimeSeconds = int64(time.Since(s.startTime).Seconds())

	return res, nil
}

// ConfigResult maps to OpenAPI ConfigResponse schema
type ConfigResult struct {
	Revision         string           `json:"revision"`
	ListenIPAddress  *string          `json:"listenIPAddress,omitempty"`
	PortBindings     []any            `json:"portBindings"`
	Users            []domain.UserSummary `json:"users"`
	LoggingLevel     *string          `json:"loggingLevel,omitempty"`
	Mtu              *int32           `json:"mtu,omitempty"`
	Dns              any              `json:"dns,omitempty"`
	TrafficPattern   any              `json:"trafficPattern,omitempty"`
	AdvancedSettings any              `json:"advancedSettings,omitempty"`
	RawJSON          string           `json:"raw_json"`
}

func (s *AdminService) GetConfig(ctx context.Context) (*ConfigResult, error) {
	rc, err := s.cfgManager.LoadConfig()
	if err != nil {
		return nil, err
	}

	cleanMap := rc.SanitizedCopy()
	cleanJSON, _ := json.MarshalIndent(cleanMap, "", "    ")

	res := &ConfigResult{
		Revision: rc.Revision(),
		Users:    rc.ListUsers(),
		RawJSON:  string(cleanJSON),
	}

	if pbVal, ok := cleanMap["portBindings"].([]any); ok {
		res.PortBindings = pbVal
	} else {
		res.PortBindings = []any{}
	}

	if ip, ok := cleanMap["listenIPAddress"].(string); ok {
		res.ListenIPAddress = &ip
	}
	if ll, ok := cleanMap["loggingLevel"].(string); ok {
		res.LoggingLevel = &ll
	}
	if mtu, ok := cleanMap["mtu"].(float64); ok {
		mtu32 := int32(mtu)
		res.Mtu = &mtu32
	}
	res.Dns = cleanMap["dns"]
	res.TrafficPattern = cleanMap["trafficPattern"]
	res.AdvancedSettings = cleanMap["advancedSettings"]

	return res, nil
}

type PreviewResult struct {
	BaseRevision      string         `json:"base_revision"`
	CandidateRevision string         `json:"candidate_revision"`
	Valid             bool           `json:"valid"`
	ValidationError   string         `json:"validation_error,omitempty"`
	Diff              string         `json:"diff"`
	PreviewConfig     map[string]any `json:"preview_config"`
}

func (s *AdminService) PreviewConfig(ctx context.Context, patchMap map[string]any) (*PreviewResult, error) {
	current, err := s.cfgManager.LoadConfig()
	if err != nil {
		return nil, err
	}

	baseRev := current.Revision()
	candidate := current.Clone()

	// Apply patch fields into candidate
	for k, v := range patchMap {
		candidate.Map()[k] = v
	}

	candidateRev := candidate.Revision()
	valErr := candidate.Validate()

	curCleanJSON, _ := json.MarshalIndent(current.SanitizedCopy(), "", "    ")
	candCleanJSON, _ := json.MarshalIndent(candidate.SanitizedCopy(), "", "    ")
	diffStr := domain.ComputeDiff(string(curCleanJSON), string(candCleanJSON))

	res := &PreviewResult{
		BaseRevision:      baseRev,
		CandidateRevision: candidateRev,
		Valid:             valErr == nil,
		Diff:              diffStr,
		PreviewConfig:     candidate.SanitizedCopy(),
	}
	if valErr != nil {
		res.ValidationError = valErr.Error()
	}

	return res, nil
}

type ApplyResult struct {
	Success  bool   `json:"success"`
	Revision string `json:"revision"`
	Message  string `json:"message"`
}

func (s *AdminService) ApplyConfig(ctx context.Context, baseRevision string, candidateMap map[string]any) (*ApplyResult, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	current, err := s.cfgManager.LoadConfig()
	if err != nil {
		return nil, fmt.Errorf("load current config failed: %w", err)
	}

	if baseRevision != "" && current.Revision() != baseRevision {
		return nil, fmt.Errorf("%w: base revision %s does not match active revision %s", domain.ErrConflict, baseRevision, current.Revision())
	}

	candidate := current.Clone()
	for k, v := range candidateMap {
		candidate.Map()[k] = v
	}

	if err := candidate.Validate(); err != nil {
		return nil, fmt.Errorf("%w: %v", domain.ErrValidationFailed, err)
	}

	// 1. Check runtime proxy state before apply
	proxyStatusKnown := false
	wasProxyRunning := false
	if s.mitaCtrl.IsInstalled() {
		_, running, err := s.mitaCtrl.GetMitaStatus()
		if err == nil {
			proxyStatusKnown = true
			wasProxyRunning = running
		}
	}

	// 2. Create native dual backup bundle (JSON and .pb)
	backupBundle, err := s.cfgManager.BackupCurrentConfig()
	if err != nil {
		return nil, fmt.Errorf("failed to create native backup before apply: %w", err)
	}

	// 3. Write candidate to temporary file with 0600 mode
	tempDir := filepath.Dir(s.configPath)
	if err := os.MkdirAll(tempDir, 0755); err != nil {
		return nil, fmt.Errorf("mkdir tempDir failed: %w", err)
	}
	tempFile := filepath.Join(tempDir, fmt.Sprintf(".candidate_%d.json", time.Now().UnixNano()))
	candJSON, err := candidate.ToJSON()
	if err != nil {
		return nil, fmt.Errorf("serialize candidate JSON failed: %w", err)
	}

	if err := os.WriteFile(tempFile, candJSON, 0600); err != nil {
		return nil, fmt.Errorf("write candidate temp file failed: %w", err)
	}
	defer os.Remove(tempFile)

	// 4. If mita is installed and systemd is active, apply config via mita replace config
	if s.mitaCtrl.IsInstalled() && s.mitaCtrl.IsSystemdActive() {
		if err := s.mitaCtrl.ReplaceConfig(tempFile); err != nil {
			// Rollback: restore dual backup and restore runtime state
			var rollbackErr error
			if rerr := s.cfgManager.RestoreBackup(backupBundle); rerr != nil {
				rollbackErr = rerr
			}
			if wasProxyRunning || !proxyStatusKnown {
				_ = s.mitaCtrl.Reload()
			} else {
				_ = s.mitaCtrl.Stop()
			}
			if rollbackErr != nil {
				return nil, fmt.Errorf("mita replace config failed (%w), AND rollback failed: %v", err, rollbackErr)
			}
			return nil, fmt.Errorf("mita replace config failed, changes rolled back: %w", err)
		}

		if wasProxyRunning || !proxyStatusKnown {
			if err := s.mitaCtrl.Reload(); err != nil {
				var rollbackErr error
				if rerr := s.cfgManager.RestoreBackup(backupBundle); rerr != nil {
					rollbackErr = rerr
				}
				if wasProxyRunning || !proxyStatusKnown {
					_ = s.mitaCtrl.Reload()
				}
				if rollbackErr != nil {
					return nil, fmt.Errorf("mita reload failed after replace (%w), AND rollback failed: %v", err, rollbackErr)
				}
				return nil, fmt.Errorf("mita reload failed after replace, changes rolled back: %w", err)
			}
		} else {
			// Do NOT silently start proxy if it was stopped before apply!
			_ = s.mitaCtrl.Stop()
		}
	}

	// 5. Save to config path (updates both JSON and native .pb)
	newRev, _, err := s.cfgManager.SaveConfig(candidate, "")
	if err != nil {
		var rollbackErr error
		if rerr := s.cfgManager.RestoreBackup(backupBundle); rerr != nil {
			rollbackErr = rerr
		}
		if wasProxyRunning || !proxyStatusKnown {
			_ = s.mitaCtrl.Reload()
		} else {
			_ = s.mitaCtrl.Stop()
		}
		if rollbackErr != nil {
			return nil, fmt.Errorf("save config to disk failed (%w), AND rollback failed: %v", err, rollbackErr)
		}
		return nil, fmt.Errorf("save config to disk failed, rolled back: %w", err)
	}

	return &ApplyResult{
		Success:  true,
		Revision: newRev,
		Message:  "Configuration applied successfully",
	}, nil
}

func (s *AdminService) ListUsers(ctx context.Context) ([]domain.UserSummary, error) {
	rc, err := s.cfgManager.LoadConfig()
	if err != nil {
		return nil, err
	}
	return rc.ListUsers(), nil
}

func (s *AdminService) AddUser(ctx context.Context, name, password string, allowPrivateIP, allowLoopbackIP *bool) (string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	current, err := s.cfgManager.LoadConfig()
	if err != nil {
		return "", err
	}

	candidate := current.Clone()
	if err := candidate.AddUser(name, password, allowPrivateIP, allowLoopbackIP); err != nil {
		return "", err
	}

	if err := candidate.Validate(); err != nil {
		return "", err
	}

	res, err := s.applyLocked(candidate)
	if err != nil {
		return "", err
	}
	return res.Revision, nil
}

func (s *AdminService) ResetUserPassword(ctx context.Context, name, newPassword string) (string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	current, err := s.cfgManager.LoadConfig()
	if err != nil {
		return "", err
	}

	candidate := current.Clone()
	if err := candidate.ResetPassword(name, newPassword); err != nil {
		return "", err
	}

	if err := candidate.Validate(); err != nil {
		return "", err
	}

	res, err := s.applyLocked(candidate)
	if err != nil {
		return "", err
	}
	return res.Revision, nil
}

func (s *AdminService) DeleteUser(ctx context.Context, name string) (string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	current, err := s.cfgManager.LoadConfig()
	if err != nil {
		return "", err
	}

	candidate := current.Clone()
	if err := candidate.DeleteUser(name); err != nil {
		return "", err
	}

	if err := candidate.Validate(); err != nil {
		return "", err
	}

	res, err := s.applyLocked(candidate)
	if err != nil {
		return "", err
	}
	return res.Revision, nil
}

func (s *AdminService) applyLocked(candidate *domain.RawConfig) (*ApplyResult, error) {
	wasProxyRunning := false
	if s.mitaCtrl.IsInstalled() {
		_, wasProxyRunning, _ = s.mitaCtrl.GetMitaStatus()
	}

	backupBundle, err := s.cfgManager.BackupCurrentConfig()
	if err != nil {
		return nil, fmt.Errorf("backup failed: %w", err)
	}

	tempDir := filepath.Dir(s.configPath)
	if err := os.MkdirAll(tempDir, 0755); err != nil {
		return nil, fmt.Errorf("mkdir tempDir failed: %w", err)
	}
	tempFile := filepath.Join(tempDir, fmt.Sprintf(".cand_%d.json", time.Now().UnixNano()))
	candJSON, _ := candidate.ToJSON()
	if err := os.WriteFile(tempFile, candJSON, 0600); err != nil {
		return nil, err
	}
	defer os.Remove(tempFile)

	if s.mitaCtrl.IsInstalled() && s.mitaCtrl.IsSystemdActive() {
		if err := s.mitaCtrl.ReplaceConfig(tempFile); err != nil {
			_ = s.cfgManager.RestoreBackup(backupBundle)
			if wasProxyRunning {
				_ = s.mitaCtrl.Reload()
			} else {
				_ = s.mitaCtrl.Stop()
			}
			return nil, fmt.Errorf("mita replace config failed: %w", err)
		}
		if wasProxyRunning {
			_ = s.mitaCtrl.Reload()
		} else {
			_ = s.mitaCtrl.Stop()
		}
	}

	newRev, _, err := s.cfgManager.SaveConfig(candidate, "")
	if err != nil {
		_ = s.cfgManager.RestoreBackup(backupBundle)
		if wasProxyRunning {
			_ = s.mitaCtrl.Reload()
		} else {
			_ = s.mitaCtrl.Stop()
		}
		return nil, err
	}

	return &ApplyResult{Success: true, Revision: newRev}, nil
}

type ServiceControlResult struct {
	Success       bool   `json:"success"`
	Action        string `json:"action"`
	SystemdActive bool   `json:"systemd_active"`
	MitaRunning   bool   `json:"mita_running"`
	Message       string `json:"message"`
}

func (s *AdminService) ControlService(ctx context.Context, action string) (*ServiceControlResult, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	var err error
	switch strings.ToLower(action) {
	case "start":
		if !s.mitaCtrl.IsSystemdActive() {
			err = s.mitaCtrl.StartSystemd()
		} else {
			err = s.mitaCtrl.Start()
		}
	case "stop":
		err = s.mitaCtrl.Stop()
	case "restart":
		err = s.mitaCtrl.Restart()
	default:
		return nil, fmt.Errorf("unsupported action %q (supported: start, stop, restart)", action)
	}

	if err != nil {
		return nil, err
	}

	sysActive := s.mitaCtrl.IsSystemdActive()
	_, proxyRun, _ := s.mitaCtrl.GetMitaStatus()

	return &ServiceControlResult{
		Success:       true,
		Action:        action,
		SystemdActive: sysActive,
		MitaRunning:   proxyRun,
		Message:       fmt.Sprintf("Service action %q executed successfully", action),
	}, nil
}

func (s *AdminService) GetLogs(ctx context.Context, lines int) ([]string, error) {
	return s.logsSvc.GetRecentLogs(lines)
}

func (s *AdminService) ExportProfile(ctx context.Context, username, password, serverHost string, portOverride int) (*domain.ClientProfile, error) {
	rc, err := s.cfgManager.LoadConfig()
	if err != nil {
		return nil, err
	}

	if serverHost == "" {
		serverHost = DetectPublicOrLocalIP()
	}

	return rc.ExportProfile(username, password, serverHost, portOverride)
}

func (s *AdminService) InstallMita(ctx context.Context, debPath string) (string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.installer.InstallDeb(debPath)
}
