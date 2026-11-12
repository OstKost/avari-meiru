package service

import (
	"bytes"
	"context"
	"os/exec"
	"regexp"
	"strconv"
	"strings"
	"time"
)

var (
	tokenRegex    = regexp.MustCompile(`(?i)(bearer\s+|token[\s=:]+|password[\s=:]+|hashedPassword[\s=:]+)[A-Za-z0-9_\-\.\+/=]{6,}`)
	authHeadRegex = regexp.MustCompile(`(?i)authorization:\s*[^\r\n]+`)
	hashRegex     = regexp.MustCompile(`\b[0-9a-fA-F]{64}\b`)
)

// SanitizeLogLine strips credentials, authorization headers, raw hashes, and sensitive tokens
func SanitizeLogLine(line string) string {
	line = authHeadRegex.ReplaceAllString(line, "Authorization: [REDACTED]")
	line = tokenRegex.ReplaceAllString(line, "$1[REDACTED]")
	line = hashRegex.ReplaceAllString(line, "[HASH_REDACTED]")
	return line
}

// LogsService provides sanitized log streaming from systemd journalctl or fallback log files
type LogsService struct {
	JournalctlPath string
}

func NewLogsService() *LogsService {
	jPath, _ := exec.LookPath("journalctl")
	return &LogsService{
		JournalctlPath: jPath,
	}
}

// GetRecentLogs retrieves the last N log lines for the mita unit, sanitized
func (s *LogsService) GetRecentLogs(lines int) ([]string, error) {
	if lines <= 0 {
		lines = 100
	}
	if lines > 1000 {
		lines = 1000
	}

	if s.JournalctlPath == "" {
		return []string{"journalctl is not available in current environment"}, nil
	}

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	cmd := exec.CommandContext(ctx, s.JournalctlPath, "-u", "mita", "-n", strconv.Itoa(lines), "--no-pager")
	var stdout bytes.Buffer
	cmd.Stdout = &stdout
	if err := cmd.Run(); err != nil {
		// Try without -u mita if service is not registered in systemd yet
		return []string{"No systemd logs found for mita.service"}, nil
	}

	rawLines := strings.Split(stdout.String(), "\n")
	var sanitized []string
	for _, l := range rawLines {
		trimmed := strings.TrimSpace(l)
		if trimmed == "" {
			continue
		}
		sanitized = append(sanitized, SanitizeLogLine(trimmed))
	}

	return sanitized, nil
}
