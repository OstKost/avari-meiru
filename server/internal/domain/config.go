package domain

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/user"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/enfein/mieru/v3/pkg/appctl"
	pb "github.com/enfein/mieru/v3/pkg/appctl/appctlpb"
	"github.com/enfein/mieru/v3/pkg/cipher"
	"github.com/enfein/mieru/v3/pkg/common"
	"google.golang.org/protobuf/proto"
)

var (
	ErrUserNotFound      = errors.New("user not found")
	ErrUserAlreadyExists = errors.New("user already exists")
	ErrInvalidPassword   = errors.New("invalid password")
	ErrConflict          = errors.New("configuration conflict")
	ErrValidationFailed  = errors.New("configuration validation failed")
	ErrUnsupportedFormat = errors.New("unsupported or corrupted configuration format")
)

// HashPassword hashes a raw password for a username using upstream mita algorithm:
// sha256(rawPassword + 0x00 + username) -> lowercase hex string
func HashPassword(rawPassword, username string) string {
	rawHash := cipher.HashPassword([]byte(rawPassword), []byte(username))
	return hex.EncodeToString(rawHash)
}

// VerifyPassword checks whether a raw password matches the stored hex hash for the user.
func VerifyPassword(rawPassword, username, storedHash string) bool {
	if storedHash == "" || rawPassword == "" || username == "" {
		return false
	}
	expected := HashPassword(rawPassword, username)
	return strings.EqualFold(expected, storedHash)
}

// FileBackup holds metadata and backup file path for atomic rollback
type FileBackup struct {
	OriginalPath string
	BackupPath   string
	Mode         os.FileMode
	Uid          int
	Gid          int
	Existed      bool
}

// BackupBundle holds snapshot of both human JSON config and native .pb config
type BackupBundle struct {
	JSONBackup *FileBackup
	PbBackup   *FileBackup
}

// ConfigManager handles loading, modifying, validating, diffing, and saving mita server config.
// It preserves unknown fields by maintaining raw JSON tree while parsing typed structures for validation.
type ConfigManager struct {
	mu           sync.RWMutex
	configPath   string
	nativePbPath string
	backupDir    string
}

func NewConfigManager(configPath string) *ConfigManager {
	if configPath == "" {
		configPath = "/etc/mita/server_config.json"
	}
	nativePb := filepath.Join(filepath.Dir(configPath), "server.conf.pb")
	backupDir := filepath.Join(filepath.Dir(configPath), "backups")
	return &ConfigManager{
		configPath:   configPath,
		nativePbPath: nativePb,
		backupDir:    backupDir,
	}
}

func (cm *ConfigManager) ConfigPath() string {
	cm.mu.RLock()
	defer cm.mu.RUnlock()
	return cm.configPath
}

func (cm *ConfigManager) NativePbPath() string {
	cm.mu.RLock()
	defer cm.mu.RUnlock()
	return cm.nativePbPath
}

func (cm *ConfigManager) SetNativePbPath(path string) {
	cm.mu.Lock()
	defer cm.mu.Unlock()
	cm.nativePbPath = path
}

// RawConfig holds the structured and unstructured fields of mita server config.
type RawConfig struct {
	rawMap map[string]any
}

// Clone creates a deep copy of rawMap
func (rc *RawConfig) Clone() *RawConfig {
	if rc == nil || rc.rawMap == nil {
		return &RawConfig{rawMap: make(map[string]any)}
	}
	b, _ := json.Marshal(rc.rawMap)
	var newMap map[string]any
	_ = json.Unmarshal(b, &newMap)
	return &RawConfig{rawMap: newMap}
}

func (rc *RawConfig) Map() map[string]any {
	if rc == nil {
		return nil
	}
	return rc.rawMap
}

func (rc *RawConfig) ToJSON() ([]byte, error) {
	if rc == nil || rc.rawMap == nil {
		return []byte("{}"), nil
	}
	return json.MarshalIndent(rc.rawMap, "", "    ")
}

// Revision returns SHA256 of canonical JSON of rawMap
func (rc *RawConfig) Revision() string {
	if rc == nil || rc.rawMap == nil {
		return ""
	}
	b, err := json.Marshal(rc.rawMap)
	if err != nil {
		return ""
	}
	h := sha256.Sum256(b)
	return hex.EncodeToString(h[:])
}

// ToProto translates the JSON map into typed protobuf ServerConfig using official upstream protojson converter.
// Preserves all schema fields (dns, trafficPattern, advancedSettings, quotas, etc.).
// Fails closed on unsupported unknown format.
func (rc *RawConfig) ToProto() (*pb.ServerConfig, error) {
	if rc == nil || rc.rawMap == nil {
		return nil, fmt.Errorf("cannot convert nil config to proto")
	}
	b, err := rc.ToJSON()
	if err != nil {
		return nil, fmt.Errorf("serialize json failed: %w", err)
	}

	cfg := &pb.ServerConfig{}
	if err := common.UnmarshalJSON(b, cfg); err != nil {
		return nil, fmt.Errorf("%w: failed to parse protobuf config: %v", ErrUnsupportedFormat, err)
	}
	return cfg, nil
}

// Validate candidate configuration using upstream appctl.ValidateFullServerConfig
func (rc *RawConfig) Validate() error {
	protoCfg, err := rc.ToProto()
	if err != nil {
		return fmt.Errorf("%w: %v", ErrValidationFailed, err)
	}
	if err := appctl.ValidateFullServerConfig(protoCfg); err != nil {
		return fmt.Errorf("%w: %v", ErrValidationFailed, err)
	}
	return nil
}

// SanitizedCopy returns a copy of rawMap with plaintext passwords and password hashes removed
func (rc *RawConfig) SanitizedCopy() map[string]any {
	cp := rc.Clone()
	if cp.rawMap == nil {
		return make(map[string]any)
	}

	if usersVal, ok := cp.rawMap["users"]; ok {
		if userList, ok := usersVal.([]any); ok {
			var sanitizedList []any
			for _, uAny := range userList {
				if uMap, ok := uAny.(map[string]any); ok {
					uClean := make(map[string]any)
					for k, v := range uMap {
						if k == "password" || k == "hashedPassword" {
							continue
						}
						uClean[k] = v
					}
					sanitizedList = append(sanitizedList, uClean)
				}
			}
			cp.rawMap["users"] = sanitizedList
		}
	}
	return cp.rawMap
}

// LoadConfig reads the authoritative configuration from disk.
// If the native .pb exists, it is primary and authoritative.
// If native .pb is absent, it falls back to active JSON override.
// Corrupted or unreadable files fail closed; empty default is returned ONLY on fresh install.
func (cm *ConfigManager) LoadConfig() (*RawConfig, error) {
	cm.mu.RLock()
	defer cm.mu.RUnlock()
	return cm.loadConfigLocked()
}

func fileSnapshot(srcPath, backupDir, prefix string) (*FileBackup, error) {
	fi, err := os.Stat(srcPath)
	if err != nil {
		if os.IsNotExist(err) {
			return &FileBackup{OriginalPath: srcPath, Existed: false}, nil
		}
		return nil, err
	}

	data, err := os.ReadFile(srcPath)
	if err != nil {
		return nil, err
	}

	fb := &FileBackup{
		OriginalPath: srcPath,
		Mode:         fi.Mode(),
		Existed:      true,
	}
	if stat, ok := fi.Sys().(*syscall.Stat_t); ok {
		fb.Uid = int(stat.Uid)
		fb.Gid = int(stat.Gid)
	}

	timestamp := time.Now().UTC().Format("20060102_150405.000000")
	backupPath := filepath.Join(backupDir, fmt.Sprintf("%s.bak.%s", prefix, timestamp))
	if err := os.WriteFile(backupPath, data, fb.Mode.Perm()); err != nil {
		return nil, err
	}
	fb.BackupPath = backupPath
	return fb, nil
}

// BackupCurrentConfig creates a private dual backup (JSON and native .pb) with owner and mode
func (cm *ConfigManager) BackupCurrentConfig() (*BackupBundle, error) {
	if err := os.MkdirAll(cm.backupDir, 0700); err != nil {
		return nil, fmt.Errorf("create backup dir failed: %w", err)
	}

	jsonBackup, err := fileSnapshot(cm.configPath, cm.backupDir, "server_config.json")
	if err != nil {
		return nil, fmt.Errorf("backup json config failed: %w", err)
	}

	pbBackup, err := fileSnapshot(cm.nativePbPath, cm.backupDir, "server.conf.pb")
	if err != nil {
		return nil, fmt.Errorf("backup native pb failed: %w", err)
	}

	return &BackupBundle{
		JSONBackup: jsonBackup,
		PbBackup:   pbBackup,
	}, nil
}

func restoreFile(fb *FileBackup) error {
	if fb == nil {
		return nil
	}
	if !fb.Existed {
		_ = os.Remove(fb.OriginalPath)
		return nil
	}
	data, err := os.ReadFile(fb.BackupPath)
	if err != nil {
		return err
	}

	dir := filepath.Dir(fb.OriginalPath)
	_ = os.MkdirAll(dir, 0755)

	perm := fb.Mode.Perm()
	if perm == 0 {
		perm = 0600
	}
	if err := os.WriteFile(fb.OriginalPath, data, perm); err != nil {
		return err
	}
	if fb.Uid != 0 || fb.Gid != 0 {
		_ = os.Chown(fb.OriginalPath, fb.Uid, fb.Gid)
	}
	return nil
}

// RestoreBackup restores both JSON and native .pb files with their original permissions and ownership
func (cm *ConfigManager) RestoreBackup(bundle *BackupBundle) error {
	if bundle == nil {
		return nil
	}
	err1 := restoreFile(bundle.JSONBackup)
	err2 := restoreFile(bundle.PbBackup)
	if err1 != nil {
		return fmt.Errorf("restore JSON failed: %w", err1)
	}
	if err2 != nil {
		return fmt.Errorf("restore pb failed: %w", err2)
	}
	return nil
}

// WriteNativePb updates /etc/mita/server.conf.pb with serialized protobuf
func (cm *ConfigManager) WriteNativePb(protoCfg *pb.ServerConfig) error {
	if protoCfg == nil {
		return nil
	}
	pbBytes, err := proto.Marshal(protoCfg)
	if err != nil {
		return fmt.Errorf("proto.Marshal failed: %w", err)
	}

	dir := filepath.Dir(cm.nativePbPath)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return fmt.Errorf("mkdir for native pb failed: %w", err)
	}

	tempFile := filepath.Join(dir, fmt.Sprintf(".server.conf.pb.tmp.%d", time.Now().UnixNano()))
	if err := os.WriteFile(tempFile, pbBytes, 0600); err != nil {
		return fmt.Errorf("write temp native pb failed: %w", err)
	}

	// Try to set owner mita:mita if user mita exists
	if u, err := user.Lookup("mita"); err == nil {
		if uid, err := strconv.Atoi(u.Uid); err == nil {
			if gid, err := strconv.Atoi(u.Gid); err == nil {
				_ = os.Chown(tempFile, uid, gid)
			}
		}
	}

	return os.Rename(tempFile, cm.nativePbPath)
}

// SaveConfig validates and writes the configuration atomically to both JSON and native .pb
func (cm *ConfigManager) SaveConfig(rc *RawConfig, expectedRevision string) (string, *BackupBundle, error) {
	cm.mu.Lock()
	defer cm.mu.Unlock()

	current, err := cm.loadConfigLocked()
	if err != nil {
		return "", nil, fmt.Errorf("cannot load current authoritative config: %w", err)
	}

	if expectedRevision != "" && current != nil && current.Revision() != expectedRevision {
		return "", nil, fmt.Errorf("%w: expected revision %s, current revision %s", ErrConflict, expectedRevision, current.Revision())
	}

	if err := rc.Validate(); err != nil {
		return "", nil, err
	}

	protoCfg, err := rc.ToProto()
	if err != nil {
		return "", nil, fmt.Errorf("convert to proto failed: %w", err)
	}

	bundle, err := cm.backupLocked()
	if err != nil {
		return "", nil, fmt.Errorf("backup failed before save: %w", err)
	}

	// 1. Write authoritative native .pb file first
	if err := cm.WriteNativePb(protoCfg); err != nil {
		if rerr := cm.RestoreBackup(bundle); rerr != nil {
			return "", bundle, fmt.Errorf("write native pb failed (%w), AND rollback failed: %v", err, rerr)
		}
		return "", bundle, fmt.Errorf("write native pb failed, rolled back: %w", err)
	}

	// 2. Write JSON mirror file atomically
	jsonData, err := rc.ToJSON()
	if err != nil {
		if rerr := cm.RestoreBackup(bundle); rerr != nil {
			return "", bundle, fmt.Errorf("serialize config failed (%w), AND rollback failed: %v", err, rerr)
		}
		return "", bundle, fmt.Errorf("serialize config failed, rolled back: %w", err)
	}

	dir := filepath.Dir(cm.configPath)
	if err := os.MkdirAll(dir, 0755); err != nil {
		if rerr := cm.RestoreBackup(bundle); rerr != nil {
			return "", bundle, fmt.Errorf("mkdir config dir failed (%w), AND rollback failed: %v", err, rerr)
		}
		return "", bundle, fmt.Errorf("mkdir config dir failed, rolled back: %w", err)
	}

	tempFile := filepath.Join(dir, fmt.Sprintf(".server_config.json.tmp.%d", time.Now().UnixNano()))
	if err := os.WriteFile(tempFile, jsonData, 0600); err != nil {
		if rerr := cm.RestoreBackup(bundle); rerr != nil {
			return "", bundle, fmt.Errorf("write temp config file failed (%w), AND rollback failed: %v", err, rerr)
		}
		return "", bundle, fmt.Errorf("write temp config file failed, rolled back: %w", err)
	}

	// Chown to mita if exists
	if u, err := user.Lookup("mita"); err == nil {
		if uid, err := strconv.Atoi(u.Uid); err == nil {
			if gid, err := strconv.Atoi(u.Gid); err == nil {
				_ = os.Chown(tempFile, uid, gid)
			}
		}
	}

	if err := os.Rename(tempFile, cm.configPath); err != nil {
		_ = os.Remove(tempFile)
		if rerr := cm.RestoreBackup(bundle); rerr != nil {
			return "", bundle, fmt.Errorf("rename temp file to config failed (%w), AND rollback failed: %v", err, rerr)
		}
		return "", bundle, fmt.Errorf("rename temp file to config failed, rolled back: %w", err)
	}

	newRev := rc.Revision()
	return newRev, bundle, nil
}

func (cm *ConfigManager) loadConfigLocked() (*RawConfig, error) {
	// 1. Authoritative check: If native .pb exists on disk, it is the primary authoritative configuration!
	pbData, pbErr := os.ReadFile(cm.nativePbPath)
	if pbErr == nil {
		if len(pbData) == 0 {
			return nil, fmt.Errorf("%w: authoritative native pb %s is empty", ErrUnsupportedFormat, cm.nativePbPath)
		}
		protoCfg := &pb.ServerConfig{}
		if err := proto.Unmarshal(pbData, protoCfg); err != nil {
			return nil, fmt.Errorf("%w: unreadable/corrupted native protobuf in %s: %v", ErrUnsupportedFormat, cm.nativePbPath, err)
		}
		jsonBytes, err := common.MarshalJSON(protoCfg)
		if err != nil {
			return nil, fmt.Errorf("%w: failed to serialize native protobuf to JSON: %v", ErrUnsupportedFormat, err)
		}
		var rawMap map[string]any
		if err := json.Unmarshal(jsonBytes, &rawMap); err != nil {
			return nil, fmt.Errorf("%w: failed to parse protobuf JSON into map: %v", ErrUnsupportedFormat, err)
		}
		return &RawConfig{rawMap: rawMap}, nil
	} else if !os.IsNotExist(pbErr) {
		return nil, fmt.Errorf("read authoritative native pb %s failed: %w", cm.nativePbPath, pbErr)
	}

	// 2. Active JSON override if native .pb does NOT exist on disk
	jsonData, jsonErr := os.ReadFile(cm.configPath)
	if jsonErr == nil {
		if len(jsonData) == 0 {
			return nil, fmt.Errorf("%w: config file %s is empty", ErrUnsupportedFormat, cm.configPath)
		}
		protoCfg := &pb.ServerConfig{}
		if err := common.UnmarshalJSON(jsonData, protoCfg); err != nil {
			return nil, fmt.Errorf("%w: config in %s does not adhere to protobuf schema: %v", ErrUnsupportedFormat, cm.configPath, err)
		}
		var rawMap map[string]any
		if err := json.Unmarshal(jsonData, &rawMap); err != nil {
			return nil, fmt.Errorf("%w: invalid JSON in %s: %v", ErrUnsupportedFormat, cm.configPath, err)
		}
		return &RawConfig{rawMap: rawMap}, nil
	} else if !os.IsNotExist(jsonErr) {
		return nil, fmt.Errorf("read config file %s failed: %w", cm.configPath, jsonErr)
	}

	// 3. ONLY if NEITHER file exists on disk, this is an explicit fresh install
	return &RawConfig{
		rawMap: map[string]any{
			"portBindings": []any{
				map[string]any{
					"port":     float64(28443),
					"protocol": "TCP",
				},
			},
			"users":        []any{},
			"loggingLevel": "INFO",
			"mtu":          float64(1400),
			"dns": map[string]any{
				"dualStack": "PREFER_IPv4",
			},
		},
	}, nil
}

func (cm *ConfigManager) backupLocked() (*BackupBundle, error) {
	if err := os.MkdirAll(cm.backupDir, 0700); err != nil {
		return nil, err
	}
	jsonBackup, err := fileSnapshot(cm.configPath, cm.backupDir, "server_config.json")
	if err != nil {
		return nil, err
	}
	pbBackup, err := fileSnapshot(cm.nativePbPath, cm.backupDir, "server.conf.pb")
	if err != nil {
		return nil, err
	}
	return &BackupBundle{
		JSONBackup: jsonBackup,
		PbBackup:   pbBackup,
	}, nil
}

// ComputeDiff produces a simple unified diff between old and new JSON representation
func ComputeDiff(oldJSON, newJSON string) string {
	oldLines := strings.Split(oldJSON, "\n")
	newLines := strings.Split(newJSON, "\n")

	var diffLines []string
	diffLines = append(diffLines, "--- current", "+++ proposed")

	i, j := 0, 0
	for i < len(oldLines) && j < len(newLines) {
		if oldLines[i] == newLines[j] {
			diffLines = append(diffLines, " "+oldLines[i])
			i++
			j++
		} else {
			diffLines = append(diffLines, "-"+oldLines[i])
			diffLines = append(diffLines, "+"+newLines[j])
			i++
			j++
		}
	}
	for ; i < len(oldLines); i++ {
		diffLines = append(diffLines, "-"+oldLines[i])
	}
	for ; j < len(newLines); j++ {
		diffLines = append(diffLines, "+"+newLines[j])
	}
	return strings.Join(diffLines, "\n")
}
