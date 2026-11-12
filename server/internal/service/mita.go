package service

import (
	"bytes"
	"context"
	"fmt"
	"os/exec"
	"strings"
	"time"
)

// MitaController defines operations performed against mita and systemd
type MitaController interface {
	IsInstalled() bool
	GetVersion() (string, error)
	IsSystemdActive() bool
	GetMitaStatus() (daemonRunning bool, proxyRunning bool, err error)
	ReplaceConfig(configFilePath string) error
	Start() error
	Stop() error
	Reload() error
	Restart() error
	StartSystemd() error
	StopSystemd() error
}

// SystemMitaController implements MitaController using system exec (systemctl, mita)
type SystemMitaController struct {
	MitaPath      string
	SystemctlPath string
}

func NewSystemMitaController() *SystemMitaController {
	mitaPath, _ := exec.LookPath("mita")
	if mitaPath == "" {
		mitaPath = "/usr/bin/mita"
	}
	sysPath, _ := exec.LookPath("systemctl")
	return &SystemMitaController{
		MitaPath:      mitaPath,
		SystemctlPath: sysPath,
	}
}

func (c *SystemMitaController) runCmd(ctx context.Context, name string, args ...string) (string, error) {
	cmd := exec.CommandContext(ctx, name, args...)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	err := cmd.Run()
	outStr := strings.TrimSpace(stdout.String())
	if err != nil {
		errStr := strings.TrimSpace(stderr.String())
		if errStr != "" {
			return outStr, fmt.Errorf("%w: %s", err, errStr)
		}
		return outStr, err
	}
	return outStr, nil
}

func (c *SystemMitaController) IsInstalled() bool {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	_, err := exec.LookPath("mita")
	if err == nil {
		return true
	}
	// Also check fixed path
	_, err = c.runCmd(ctx, c.MitaPath, "version")
	return err == nil
}

func (c *SystemMitaController) GetVersion() (string, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	out, err := c.runCmd(ctx, c.MitaPath, "version")
	if err != nil {
		return "", err
	}
	// Output is typically like "mita version 3.38.0" or "3.38.0"
	lines := strings.Split(out, "\n")
	if len(lines) > 0 {
		return strings.TrimSpace(lines[0]), nil
	}
	return out, nil
}

func (c *SystemMitaController) IsSystemdActive() bool {
	if c.SystemctlPath == "" {
		return false
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	out, err := c.runCmd(ctx, c.SystemctlPath, "is-active", "mita")
	if err != nil {
		return false
	}
	return strings.TrimSpace(out) == "active"
}

func (c *SystemMitaController) GetMitaStatus() (bool, bool, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	out, err := c.runCmd(ctx, c.MitaPath, "status")
	if err != nil {
		// If connection refused, daemon is not running
		if strings.Contains(err.Error(), "connection refused") || strings.Contains(out, "connection refused") {
			return false, false, nil
		}
		// If daemon is running but proxy is not running:
		if strings.Contains(out, "proxy is not running") || strings.Contains(err.Error(), "proxy is not running") {
			return true, false, nil
		}
		return false, false, err
	}
	if strings.Contains(out, "RUNNING") {
		return true, true, nil
	}
	return true, false, nil
}

func (c *SystemMitaController) ReplaceConfig(configFilePath string) error {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	_, err := c.runCmd(ctx, c.MitaPath, "replace", "config", configFilePath)
	return err
}

func (c *SystemMitaController) Start() error {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_, err := c.runCmd(ctx, c.MitaPath, "start")
	return err
}

func (c *SystemMitaController) Stop() error {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_, err := c.runCmd(ctx, c.MitaPath, "stop")
	return err
}

func (c *SystemMitaController) Reload() error {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	_, err := c.runCmd(ctx, c.MitaPath, "reload")
	return err
}

func (c *SystemMitaController) StartSystemd() error {
	if c.SystemctlPath == "" {
		return fmt.Errorf("systemctl not found")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	_, err := c.runCmd(ctx, c.SystemctlPath, "start", "mita")
	return err
}

func (c *SystemMitaController) StopSystemd() error {
	if c.SystemctlPath == "" {
		return fmt.Errorf("systemctl not found")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	_, err := c.runCmd(ctx, c.SystemctlPath, "stop", "mita")
	return err
}

func (c *SystemMitaController) Restart() error {
	if c.SystemctlPath != "" && c.IsSystemdActive() {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		_, err := c.runCmd(ctx, c.SystemctlPath, "restart", "mita")
		return err
	}
	_ = c.Stop()
	return c.Start()
}
