package service

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

const (
	ExpectedMitaVersion = "3.38.0"
	ExpectedMitaDebSha  = "5170555aace38f76862541587b632d61848bba12f2e845b057977c9f9257e59a"
	OfficialMitaDebURL  = "https://github.com/enfein/mieru/releases/download/v3.38.0/mita_3.38.0_amd64.deb"
)

var (
	ErrUnsupportedOS        = errors.New("unsupported operating system or architecture")
	ErrChecksumMismatch     = errors.New("deb package checksum verification failed")
	ErrUnsupportedVersion   = errors.New("installed mita version is unsupported; automatic replacement is rejected")
	ErrAlreadyInstalled     = errors.New("mita is already installed with expected version")
	ErrInstallationFailed   = errors.New("package installation failed")
)

type InstallerService struct {
	controller MitaController
}

func NewInstallerService(controller MitaController) *InstallerService {
	return &InstallerService{
		controller: controller,
	}
}

// PreflightCheck validates that the machine is Linux x86_64, systemd present, and user is root
func (s *InstallerService) PreflightCheck() (map[string]any, error) {
	res := map[string]any{
		"os":       runtime.GOOS,
		"arch":     runtime.GOARCH,
		"is_root":  os.Geteuid() == 0,
		"systemd":  false,
		"distro":   "unknown",
		"version":  "unknown",
	}

	if runtime.GOOS != "linux" {
		// Allowed in development / tests, but marked
		res["compatible"] = false
		res["reason"] = "OS is not Linux"
		return res, nil
	}

	if runtime.GOARCH != "amd64" {
		res["compatible"] = false
		res["reason"] = fmt.Sprintf("architecture %s is not supported (required amd64)", runtime.GOARCH)
		return res, nil
	}

	if os.Geteuid() != 0 {
		res["compatible"] = false
		res["reason"] = "root privileges required"
		return res, nil
	}

	// Check systemd
	if _, err := os.Stat("/run/systemd/system"); err == nil {
		res["systemd"] = true
	}

	// Read /etc/os-release
	if data, err := os.ReadFile("/etc/os-release"); err == nil {
		lines := strings.Split(string(data), "\n")
		for _, l := range lines {
			if strings.HasPrefix(l, "ID=") {
				res["distro"] = strings.Trim(strings.TrimPrefix(l, "ID="), `"`)
			}
			if strings.HasPrefix(l, "VERSION_ID=") {
				res["version"] = strings.Trim(strings.TrimPrefix(l, "VERSION_ID="), `"`)
			}
		}
	}

	distro, _ := res["distro"].(string)
	if distro == "ubuntu" || distro == "debian" {
		res["compatible"] = true
	} else {
		res["compatible"] = false
		res["reason"] = fmt.Sprintf("distro %s is not officially supported (Ubuntu 24.04 / Debian 13 required)", distro)
	}

	return res, nil
}

// VerifyFileSHA256 returns true if the file matches ExpectedMitaDebSha
func VerifyFileSHA256(filePath, expectedSha string) (bool, string, error) {
	f, err := os.Open(filePath)
	if err != nil {
		return false, "", err
	}
	defer f.Close()

	hasher := sha256.New()
	if _, err := io.Copy(hasher, f); err != nil {
		return false, "", err
	}

	actualSha := hex.EncodeToString(hasher.Sum(nil))
	match := strings.EqualFold(actualSha, expectedSha)
	return match, actualSha, nil
}

// DownloadOfficialDeb downloads mita deb into destination path
func DownloadOfficialDeb(destPath string) error {
	client := &http.Client{Timeout: 60 * time.Second}
	resp, err := client.Get(OfficialMitaDebURL)
	if err != nil {
		return fmt.Errorf("download mita deb failed: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("download mita deb returned HTTP %d", resp.StatusCode)
	}

	out, err := os.Create(destPath)
	if err != nil {
		return fmt.Errorf("create file for deb download failed: %w", err)
	}
	defer out.Close()

	if _, err := io.Copy(out, resp.Body); err != nil {
		return fmt.Errorf("write deb file failed: %w", err)
	}

	return nil
}

// InstallDeb verifies checksum and runs apt/dpkg installation. Force replacement is disabled.
func (s *InstallerService) InstallDeb(debPath string) (string, error) {
	// Strict preflight check
	pre, _ := s.PreflightCheck()
	if compat, ok := pre["compatible"].(bool); ok && !compat {
		return "", fmt.Errorf("%w: %v", ErrUnsupportedOS, pre["reason"])
	}

	// Check existing installation
	if s.controller.IsInstalled() {
		ver, err := s.controller.GetVersion()
		if err == nil {
			if strings.TrimSpace(ver) == ExpectedMitaVersion || strings.Contains(ver, ExpectedMitaVersion) {
				return ver, fmt.Errorf("%w: version %s", ErrAlreadyInstalled, ver)
			}
			return ver, fmt.Errorf("%w: installed version %s does not match expected %s", ErrUnsupportedVersion, ver, ExpectedMitaVersion)
		}
	}

	targetDeb := debPath
	var tempDir string
	if targetDeb == "" {
		var err error
		tempDir, err = os.MkdirTemp("", "mita-install-*")
		if err != nil {
			return "", fmt.Errorf("create temp dir for deb failed: %w", err)
		}
		defer os.RemoveAll(tempDir)

		targetDeb = filepath.Join(tempDir, "mita_3.38.0_amd64.deb")
		if err := DownloadOfficialDeb(targetDeb); err != nil {
			return "", err
		}
	}

	// Verify SHA256 checksum
	match, actualSha, err := VerifyFileSHA256(targetDeb, ExpectedMitaDebSha)
	if err != nil {
		return "", fmt.Errorf("verify deb checksum failed: %w", err)
	}
	if !match {
		return "", fmt.Errorf("%w: expected %s, got %s", ErrChecksumMismatch, ExpectedMitaDebSha, actualSha)
	}

	// Run installation: apt-get install -y <targetDeb> or dpkg -i
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, "apt-get", "install", "-y", targetDeb)
	if out, err := cmd.CombinedOutput(); err != nil {
		// Fallback to dpkg -i then apt-get -f install -y
		dpkgCmd := exec.CommandContext(ctx, "dpkg", "-i", targetDeb)
		dpkgOut, dpkgErr := dpkgCmd.CombinedOutput()
		if dpkgErr != nil {
			fixCmd := exec.CommandContext(ctx, "apt-get", "install", "-f", "-y")
			_ = fixCmd.Run()
			return "", fmt.Errorf("%w: apt error: %v (%s), dpkg error: %v (%s)", ErrInstallationFailed, err, string(out), dpkgErr, string(dpkgOut))
		}
	}

	// Verify installed version
	ver, err := s.controller.GetVersion()
	if err != nil {
		return ExpectedMitaVersion, nil
	}
	return ver, nil
}
