package domain

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"

	pb "github.com/enfein/mieru/v3/pkg/appctl/appctlpb"
	"google.golang.org/protobuf/proto"
)

func TestHashAndVerifyPassword(t *testing.T) {
	username := "user@example.com"
	password := "SecretPass123!"

	hash := HashPassword(password, username)
	if hash == "" {
		t.Fatalf("expected non-empty hash")
	}

	if !VerifyPassword(password, username, hash) {
		t.Errorf("expected password verification to succeed")
	}

	if VerifyPassword("WrongPass", username, hash) {
		t.Errorf("expected wrong password verification to fail")
	}

	if VerifyPassword(password, "other@example.com", hash) {
		t.Errorf("expected different username verification to fail")
	}
}

func TestConfigPreservesAllSchemaFields(t *testing.T) {
	tmpDir := t.TempDir()
	confPath := filepath.Join(tmpDir, "server_config.json")

	initialJSON := `{
    "portBindings": [
        {"port": 28443, "protocol": "TCP"},
        {"port": 9000, "protocol": "UDP"}
    ],
    "users": [
        {"name": "test1@example.com", "hashedPassword": "abcd"}
    ],
    "loggingLevel": "INFO",
    "mtu": 1400,
    "dns": {
        "dualStack": "PREFER_IPv4"
    }
}`

	if err := os.WriteFile(confPath, []byte(initialJSON), 0600); err != nil {
		t.Fatalf("write initial file: %v", err)
	}

	cm := NewConfigManager(confPath)
	rc, err := cm.LoadConfig()
	if err != nil {
		t.Fatalf("LoadConfig failed: %v", err)
	}

	// Add user
	err = rc.AddUser("test2@example.com", "Pass456!", nil, nil)
	if err != nil {
		t.Fatalf("AddUser failed: %v", err)
	}

	// Change TCP port
	err = rc.SetPrimaryTCPPort(38443)
	if err != nil {
		t.Fatalf("SetPrimaryTCPPort failed: %v", err)
	}

	rev := rc.Revision()
	newRev, backupBundle, err := cm.SaveConfig(rc, "")
	if err != nil {
		t.Fatalf("SaveConfig failed: %v", err)
	}
	if newRev != rev {
		t.Errorf("expected rev %s, got %s", rev, newRev)
	}
	if backupBundle == nil || backupBundle.JSONBackup == nil || backupBundle.JSONBackup.BackupPath == "" {
		t.Errorf("expected backup bundle to be created")
	}

	// Re-load and verify all fields are preserved
	reloaded, err := cm.LoadConfig()
	if err != nil {
		t.Fatalf("re-load failed: %v", err)
	}

	// Check that UDP port 9000 was preserved
	bindings := reloaded.rawMap["portBindings"].([]any)
	if len(bindings) != 2 {
		t.Errorf("expected 2 port bindings, got %d", len(bindings))
	}

	// Check primary port updated to 38443
	if reloaded.GetPrimaryTCPPort() != 38443 {
		t.Errorf("expected primary TCP port 38443, got %d", reloaded.GetPrimaryTCPPort())
	}

	// Check users
	users := reloaded.ListUsers()
	if len(users) != 2 {
		t.Errorf("expected 2 users, got %d", len(users))
	}
}

func TestConflictDetection(t *testing.T) {
	tmpDir := t.TempDir()
	confPath := filepath.Join(tmpDir, "server_config.json")

	cm := NewConfigManager(confPath)
	rc, err := cm.LoadConfig()
	if err != nil {
		t.Fatalf("LoadConfig failed: %v", err)
	}

	_ = rc.AddUser("u1@example.com", "pass1", nil, nil)
	rev1, _, err := cm.SaveConfig(rc, "")
	if err != nil {
		t.Fatalf("first save failed: %v", err)
	}

	// Simulate client A and client B loading at rev1
	clientA, _ := cm.LoadConfig()
	clientB, _ := cm.LoadConfig()

	_ = clientA.AddUser("u2@example.com", "pass2", nil, nil)
	revA, _, err := cm.SaveConfig(clientA, rev1)
	if err != nil {
		t.Fatalf("client A save failed: %v", err)
	}

	// Client B tries to save with stale rev1
	_ = clientB.AddUser("u3@example.com", "pass3", nil, nil)
	_, _, err = cm.SaveConfig(clientB, rev1)
	if err == nil {
		t.Fatalf("expected conflict error for client B, got nil")
	}

	// Client B re-reads and applies with revA
	fresh, _ := cm.LoadConfig()
	_ = fresh.AddUser("u3@example.com", "pass3", nil, nil)
	_, _, err = cm.SaveConfig(fresh, revA)
	if err != nil {
		t.Fatalf("fresh save failed: %v", err)
	}
}

func TestProfileExport(t *testing.T) {
	rc := &RawConfig{
		rawMap: map[string]any{
			"portBindings": []any{
				map[string]any{"port": float64(28443), "protocol": "TCP"},
			},
			"users": []any{},
		},
	}

	err := rc.AddUser("admin@mieru.io", "StrongSecretPass!", nil, nil)
	if err != nil {
		t.Fatalf("AddUser: %v", err)
	}

	// Export with right password
	profile, err := rc.ExportProfile("admin@mieru.io", "StrongSecretPass!", "31.58.213.192", 0)
	if err != nil {
		t.Fatalf("ExportProfile failed: %v", err)
	}

	if profile.Server != "31.58.213.192" || profile.ServerPort != 28443 || profile.Transport != "TCP" {
		t.Errorf("unexpected profile data: %+v", profile)
	}
	if profile.Username != "admin@mieru.io" || profile.Password != "StrongSecretPass!" {
		t.Errorf("unexpected credentials in profile: %+v", profile)
	}

	// Export with wrong password must fail
	_, err = rc.ExportProfile("admin@mieru.io", "WrongPass", "31.58.213.192", 0)
	if err == nil {
		t.Fatalf("expected error for wrong password, got nil")
	}

	// Export with non-existent user must fail
	_, err = rc.ExportProfile("nonexistent@mieru.io", "pass", "31.58.213.192", 0)
	if err == nil {
		t.Fatalf("expected error for nonexistent user, got nil")
	}
}

func TestSanitizedCopy(t *testing.T) {
	rc := &RawConfig{
		rawMap: map[string]any{
			"users": []any{
				map[string]any{
					"name":           "u1",
					"password":       "plaintext",
					"hashedPassword": "hash",
					"custom":         "keep",
				},
			},
		},
	}

	clean := rc.SanitizedCopy()
	users := clean["users"].([]any)
	u1 := users[0].(map[string]any)

	if _, ok := u1["password"]; ok {
		t.Errorf("sanitized copy still contains plaintext password")
	}
	if _, ok := u1["hashedPassword"]; ok {
		t.Errorf("sanitized copy still contains hashedPassword")
	}
	if u1["custom"] != "keep" || u1["name"] != "u1" {
		t.Errorf("sanitized copy lost non-secret fields")
	}
}

func TestRollbackAndRestore(t *testing.T) {
	tmpDir := t.TempDir()
	confPath := filepath.Join(tmpDir, "server_config.json")

	initial := `{"portBindings": [{"port": 28443, "protocol": "TCP"}], "users": []}`
	_ = os.WriteFile(confPath, []byte(initial), 0600)

	cm := NewConfigManager(confPath)
	backupPath, err := cm.BackupCurrentConfig()
	if err != nil {
		t.Fatalf("backup failed: %v", err)
	}

	// Corrupt or change file
	_ = os.WriteFile(confPath, []byte(`{"corrupted": true}`), 0600)

	// Restore
	err = cm.RestoreBackup(backupPath)
	if err != nil {
		t.Fatalf("restore failed: %v", err)
	}

	restoredData, _ := os.ReadFile(confPath)
	var parsed map[string]any
	_ = json.Unmarshal(restoredData, &parsed)
	if _, ok := parsed["portBindings"]; !ok {
		t.Errorf("expected portBindings to be restored")
	}
}

func TestConfigIntegrity_StaleJSONWithModifiedNative(t *testing.T) {
	tmpDir := t.TempDir()
	confPath := filepath.Join(tmpDir, "server_config.json")
	nativePbPath := filepath.Join(tmpDir, "server.conf.pb")

	cm := NewConfigManager(confPath)
	cm.SetNativePbPath(nativePbPath)

	// 1. Authoritative native .pb exists with native_user
	authoritativeCfg := &pb.ServerConfig{
		PortBindings: []*pb.PortBinding{
			{Port: proto.Int32(28443), Protocol: pb.TransportProtocol_TCP.Enum()},
		},
		Users: []*pb.User{
			{Name: proto.String("native_user@mieru.io"), HashedPassword: proto.String("native_hash")},
		},
	}
	pbBytes, err := proto.Marshal(authoritativeCfg)
	if err != nil {
		t.Fatalf("proto.Marshal failed: %v", err)
	}
	if err := os.WriteFile(nativePbPath, pbBytes, 0600); err != nil {
		t.Fatalf("write native pb failed: %v", err)
	}

	// 2. A stale JSON mirror also exists on disk with old_user
	staleJSON := `{
    "portBindings": [{"port": 28443, "protocol": "TCP"}],
    "users": [{"name": "stale_old_user@mieru.io", "hashedPassword": "stale_hash"}]
}`
	if err := os.WriteFile(confPath, []byte(staleJSON), 0600); err != nil {
		t.Fatalf("write stale json failed: %v", err)
	}

	// 3. LoadConfig MUST load authoritative native .pb, NOT stale JSON!
	loaded, err := cm.LoadConfig()
	if err != nil {
		t.Fatalf("LoadConfig failed: %v", err)
	}
	users := loaded.ListUsers()
	if len(users) != 1 || users[0].Name != "native_user@mieru.io" {
		t.Fatalf("expected authoritative user 'native_user@mieru.io', got: %+v", users)
	}

	// 4. Stale JSON revision passed to SaveConfig must be rejected with ErrConflict
	staleRev := (&RawConfig{rawMap: map[string]any{
		"portBindings": []any{map[string]any{"port": float64(28443), "protocol": "TCP"}},
		"users": []any{map[string]any{"name": "stale_old_user@mieru.io", "hashedPassword": "stale_hash"}},
	}}).Revision()

	candidate := loaded.Clone()
	_ = candidate.AddUser("new_user@mieru.io", "Pass123!", nil, nil)

	_, _, err = cm.SaveConfig(candidate, staleRev)
	if !errors.Is(err, ErrConflict) {
		t.Fatalf("expected ErrConflict when saving with stale revision, got: %v", err)
	}

	// 5. Saving with current revision succeeds and preserves native user
	_, _, err = cm.SaveConfig(candidate, loaded.Revision())
	if err != nil {
		t.Fatalf("SaveConfig with active revision failed: %v", err)
	}

	reloaded, err := cm.LoadConfig()
	if err != nil {
		t.Fatalf("re-load failed: %v", err)
	}
	reloadedUsers := reloaded.ListUsers()
	if len(reloadedUsers) != 2 {
		t.Fatalf("expected 2 users preserved, got: %d", len(reloadedUsers))
	}
}

func TestConfigIntegrity_CorruptedPBNoWrite(t *testing.T) {
	tmpDir := t.TempDir()
	confPath := filepath.Join(tmpDir, "server_config.json")
	nativePbPath := filepath.Join(tmpDir, "server.conf.pb")

	cm := NewConfigManager(confPath)
	cm.SetNativePbPath(nativePbPath)

	// Write garbage corrupted bytes to native .pb
	corruptBytes := []byte{0xff, 0xfe, 0x00, 0x01, 0x88, 0x99, 0xaa}
	if err := os.WriteFile(nativePbPath, corruptBytes, 0600); err != nil {
		t.Fatalf("write corrupt pb failed: %v", err)
	}

	// LoadConfig MUST fail with ErrUnsupportedFormat and NEVER silently return empty default!
	_, err := cm.LoadConfig()
	if err == nil {
		t.Fatalf("expected error loading corrupted PB, got nil")
	}
	if !errors.Is(err, ErrUnsupportedFormat) {
		t.Errorf("expected ErrUnsupportedFormat, got: %v", err)
	}

	// Attempting to SaveConfig over corrupted PB must fail and NOT overwrite silently
	freshConfig, _ := NewConfigManager(filepath.Join(tmpDir, "nonexistent.json")).LoadConfig()
	_, _, saveErr := cm.SaveConfig(freshConfig, "")
	if saveErr == nil {
		t.Fatalf("expected SaveConfig to fail when active config is corrupted, got nil")
	}

	// Verify corrupted PB was NOT overwritten
	currentPbBytes, _ := os.ReadFile(nativePbPath)
	if string(currentPbBytes) != string(corruptBytes) {
		t.Errorf("corrupted PB was overwritten despite failure!")
	}
}

func TestConfigIntegrity_RoundtripAllSchemaFields(t *testing.T) {
	tmpDir := t.TempDir()
	confPath := filepath.Join(tmpDir, "server_config.json")
	nativePbPath := filepath.Join(tmpDir, "server.conf.pb")

	cm := NewConfigManager(confPath)
	cm.SetNativePbPath(nativePbPath)

	// Create comprehensive ServerConfig with DNS, TrafficPattern, AdvancedSettings, Quotas
	originalCfg := &pb.ServerConfig{
		ListenIPAddress: proto.String("127.0.0.1"),
		PortBindings: []*pb.PortBinding{
			{Port: proto.Int32(28443), Protocol: pb.TransportProtocol_TCP.Enum()},
			{Port: proto.Int32(9000), Protocol: pb.TransportProtocol_UDP.Enum()},
		},
		Users: []*pb.User{
			{
				Name:           proto.String("quota_user@mieru.io"),
				HashedPassword: proto.String("a1b2c3d4e5f6"),
				Quotas: []*pb.Quota{
					{Days: proto.Int32(30), Megabytes: proto.Int32(50000)},
				},
				AllowPrivateIP:  proto.Bool(true),
				AllowLoopbackIP: proto.Bool(false),
			},
		},
		LoggingLevel: pb.LoggingLevel_DEBUG.Enum(),
		Mtu:          proto.Int32(1350),
		Dns: &pb.DNS{
			DualStack: pb.DualStack_PREFER_IPv4.Enum(),
			Hosts: map[string]string{
				"internal.service": "10.0.0.1",
			},
		},
		TrafficPattern: &pb.TrafficPattern{
			Seed:      proto.Int32(12345),
			UnlockAll: proto.Bool(true),
		},
		AdvancedSettings: &pb.ServerAdvancedSettings{
			UserHintIsMandatory:    proto.Bool(true),
			MetricsLoggingInterval: proto.String("5m"),
		},
	}

	pbBytes, err := proto.Marshal(originalCfg)
	if err != nil {
		t.Fatalf("proto.Marshal failed: %v", err)
	}
	if err := os.WriteFile(nativePbPath, pbBytes, 0600); err != nil {
		t.Fatalf("write native pb failed: %v", err)
	}

	// Load via ConfigManager
	loaded, err := cm.LoadConfig()
	if err != nil {
		t.Fatalf("LoadConfig failed: %v", err)
	}

	// Convert back to proto
	roundtripProto, err := loaded.ToProto()
	if err != nil {
		t.Fatalf("ToProto failed: %v", err)
	}

	// Verify exact protobuf equality
	if !proto.Equal(originalCfg, roundtripProto) {
		t.Errorf("roundtrip proto mismatch!\nExpected: %+v\nGot:      %+v", originalCfg, roundtripProto)
	}

	// Verify unsupported unknown format fails closed
	loaded.Map()["unknown_future_field"] = "bad_value"
	_, err = loaded.ToProto()
	if err == nil {
		t.Fatalf("expected ToProto to fail closed on unknown format, got nil")
	}
	if !errors.Is(err, ErrUnsupportedFormat) {
		t.Errorf("expected ErrUnsupportedFormat on unknown format, got: %v", err)
	}
}

func TestConfigIntegrity_FailureNativeWriteRollback(t *testing.T) {
	tmpDir := t.TempDir()
	confPath := filepath.Join(tmpDir, "server_config.json")
	nativePbPath := filepath.Join(tmpDir, "server.conf.pb")

	cm := NewConfigManager(confPath)
	cm.SetNativePbPath(nativePbPath)

	// Save initial valid config
	initCfg, _ := cm.LoadConfig()
	rev0, _, err := cm.SaveConfig(initCfg, "")
	if err != nil {
		t.Fatalf("initial SaveConfig failed: %v", err)
	}

	initialJSONBytes, _ := os.ReadFile(confPath)
	initialPBBytes, _ := os.ReadFile(nativePbPath)

	// Now make native PB path unwritable (directory that cannot be written into)
	unwritableDir := filepath.Join(tmpDir, "readonly_dir")
	_ = os.MkdirAll(unwritableDir, 0500)
	cm.SetNativePbPath(filepath.Join(unwritableDir, "sub", "server.conf.pb"))

	candidate := initCfg.Clone()
	_ = candidate.AddUser("rollback_test@mieru.io", "Pass123!", nil, nil)

	_, _, err = cm.SaveConfig(candidate, rev0)
	if err == nil {
		t.Fatalf("expected SaveConfig to fail due to unwritable native PB, got nil")
	}

	// Verify original JSON was restored / kept intact by rollback
	currentJSONBytes, _ := os.ReadFile(confPath)
	if string(currentJSONBytes) != string(initialJSONBytes) {
		t.Errorf("JSON mirror was not restored by rollback on native PB write failure!")
	}

	cm.SetNativePbPath(nativePbPath)
	currentPBBytes, _ := os.ReadFile(nativePbPath)
	if string(currentPBBytes) != string(initialPBBytes) {
		t.Errorf("native PB was corrupted on write failure!")
	}
}
