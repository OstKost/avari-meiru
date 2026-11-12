package main

import (
	"context"
	"flag"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"time"

	tea "github.com/charmbracelet/bubbletea"
	"mieru-admin/internal/domain"
	"mieru-admin/internal/service"
	apihttp "mieru-admin/internal/transport/http"
	"mieru-admin/internal/tui"
)

var (
	Version   = "1.0.0"
	Commit    = "dev"
	BuildTime = time.Now().UTC().Format(time.RFC3339)
)

const systemdUnitContent = `[Unit]
Description=Mieru Admin Management Daemon
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/mieru-admin serve
Restart=always
RestartSec=5
User=root
LimitNOFILE=65535
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
`

func main() {
	if len(os.Args) < 2 {
		printUsage()
		os.Exit(1)
	}

	command := os.Args[1]
	args := os.Args[2:]

	switch command {
	case "setup":
		runSetup(args)
	case "serve":
		runServe(args)
	case "tui":
		runTUI(args)
	case "version", "--version", "-v":
		runVersion()
	case "help", "--help", "-h":
		printUsage()
	default:
		fmt.Fprintf(os.Stderr, "Unknown command: %s\n\n", command)
		printUsage()
		os.Exit(1)
	}
}

func printUsage() {
	fmt.Printf(`Mieru Admin CLI (%s) - Management daemon & TUI for Mieru (mita)

Usage:
  mieru-admin <command> [flags]

Commands:
  setup    Install daemon binary, token, systemd unit and start service (requires root)
  serve    Run management HTTP API daemon on 127.0.0.1:9081
  tui      Launch interactive terminal user interface
  version  Show version and build info

Flags for 'setup':
  --no-tui          Do not launch TUI after successful setup
  --token-file      Custom token file path (default: /etc/mieru-admin/token)
  --config-file     Custom server config path (default: /etc/mita/server_config.json)

Flags for 'serve':
  --listen          Listen address (strictly loopback, default: 127.0.0.1:9081)
  --token-file      Token file path (default: /etc/mieru-admin/token)
  --config-file     Server config path (default: /etc/mita/server_config.json)

Flags for 'tui':
  --api-url         Mieru Admin API URL (default: http://127.0.0.1:9081/v1)
  --token-file      Token file to read from under sudo/root (default: /etc/mieru-admin/token)

`, Version)
}

func runVersion() {
	fmt.Printf("mieru-admin version %s (%s)\n", Version, Commit)
	fmt.Printf("built at: %s\n", BuildTime)
	fmt.Printf("go version: %s\n", runtime.Version())
	fmt.Printf("os/arch: %s/%s\n", runtime.GOOS, runtime.GOARCH)
}

func runSetup(args []string) {
	fs := flag.NewFlagSet("setup", flag.ExitOnError)
	noTUI := fs.Bool("no-tui", false, "Do not launch TUI after setup")
	tokenPath := fs.String("token-file", "/etc/mieru-admin/token", "Path to token file")
	configPath := fs.String("config-file", "/etc/mita/server_config.json", "Path to config file")
	_ = fs.Parse(args)

	fmt.Println("=== Mieru Admin Setup ===")

	// 1. Check root privileges
	if os.Geteuid() != 0 && runtime.GOOS == "linux" {
		fmt.Fprintf(os.Stderr, "Error: setup command requires root privileges (sudo mieru-admin setup)\n")
		os.Exit(1)
	}

	// 2. Preflight verification for Linux: architecture, OS distro, real systemd
	if runtime.GOOS == "linux" {
		if runtime.GOARCH != "amd64" {
			fmt.Fprintf(os.Stderr, "Error: unsupported architecture %s (amd64 required)\n", runtime.GOARCH)
			os.Exit(1)
		}

		distro := ""
		if data, err := os.ReadFile("/etc/os-release"); err == nil {
			lines := strings.Split(string(data), "\n")
			for _, l := range lines {
				if strings.HasPrefix(l, "ID=") {
					distro = strings.Trim(strings.TrimPrefix(l, "ID="), `"`)
				}
			}
		}
		if distro != "ubuntu" && distro != "debian" {
			fmt.Fprintf(os.Stderr, "Error: unsupported distribution %q (Ubuntu 24.04 or Debian 13 required)\n", distro)
			os.Exit(1)
		}

		// Verify real systemd
		if _, err := os.Stat("/run/systemd/system"); err != nil {
			fmt.Fprintf(os.Stderr, "Error: systemd is required but not active (/run/systemd/system missing)\n")
			os.Exit(1)
		}
	}

	// 3. Install binary to /usr/local/bin/mieru-admin
	selfPath, err := os.Executable()
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error: unable to determine executable path: %v\n", err)
		os.Exit(1)
	}
	targetBin := "/usr/local/bin/mieru-admin"
	if selfPath != targetBin {
		fmt.Printf("Installing binary to %s...\n", targetBin)
		if err := copyBinary(selfPath, targetBin); err != nil {
			fmt.Fprintf(os.Stderr, "Error: failed to copy binary to %s: %v\n", targetBin, err)
			os.Exit(1)
		}
		fmt.Printf("Binary installed to %s successfully\n", targetBin)
	}

	// 4. Generate or ensure token exists with 0600 mode and root ownership
	fmt.Printf("Configuring authentication token at %s...\n", *tokenPath)
	_, err = apihttp.GenerateOrReadToken(*tokenPath)
	if err != nil {
		fmt.Fprintf(os.Stderr, "Error configuring token: %v\n", err)
		os.Exit(1)
	}
	fmt.Println("Authentication token configured (permissions 0600, root owned)")

	// 5. Ensure config directory exists with 0755 permissions
	confDir := filepath.Dir(*configPath)
	if err := os.MkdirAll(confDir, 0755); err != nil {
		fmt.Fprintf(os.Stderr, "Error: failed to create config directory %s: %v\n", confDir, err)
		os.Exit(1)
	}

	// 6. Install systemd unit (refusing unexpected foreign units)
	unitPath := "/etc/systemd/system/mieru-admin.service"
	if runtime.GOOS == "linux" {
		if existing, err := os.ReadFile(unitPath); err == nil {
			if !strings.Contains(string(existing), "Description=Mieru Admin Management Daemon") {
				fmt.Fprintf(os.Stderr, "Error: unexpected foreign unit found at %s. Refusing to overwrite.\n", unitPath)
				os.Exit(1)
			}
		}

		unitContent := fmt.Sprintf(`[Unit]
Description=Mieru Admin Management Daemon
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/mieru-admin serve --token-file=%s --config-file=%s
Restart=always
RestartSec=5
User=root
LimitNOFILE=65535
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
`, *tokenPath, *configPath)

		fmt.Printf("Installing systemd unit at %s...\n", unitPath)
		if err := os.WriteFile(unitPath, []byte(unitContent), 0644); err != nil {
			fmt.Fprintf(os.Stderr, "Error: failed to write systemd unit: %v\n", err)
			os.Exit(1)
		}

		if out, err := exec.Command("systemctl", "daemon-reload").CombinedOutput(); err != nil {
			fmt.Fprintf(os.Stderr, "Error: systemctl daemon-reload failed: %v (%s)\n", err, string(out))
			os.Exit(1)
		}
		if out, err := exec.Command("systemctl", "enable", "mieru-admin").CombinedOutput(); err != nil {
			fmt.Fprintf(os.Stderr, "Error: systemctl enable mieru-admin failed: %v (%s)\n", err, string(out))
			os.Exit(1)
		}
		if out, err := exec.Command("systemctl", "restart", "mieru-admin").CombinedOutput(); err != nil {
			fmt.Fprintf(os.Stderr, "Error: systemctl restart mieru-admin failed: %v (%s)\n", err, string(out))
			os.Exit(1)
		}

		// Verify service readiness
		time.Sleep(1 * time.Second)
		activeOut, err := exec.Command("systemctl", "is-active", "mieru-admin").CombinedOutput()
		if err != nil || strings.TrimSpace(string(activeOut)) != "active" {
			fmt.Fprintf(os.Stderr, "Error: mieru-admin service is not active after start (status: %s)\n", strings.TrimSpace(string(activeOut)))
			os.Exit(1)
		}
		fmt.Println("Systemd service enabled, started, and verified active")
	}

	fmt.Println("Setup completed successfully!")

	if !*noTUI {
		fmt.Println("Launching TUI...")
		runTUI([]string{
			"--token-file=" + *tokenPath,
		})
	}
}

func copyBinary(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()

	if err := os.MkdirAll(filepath.Dir(dst), 0755); err != nil {
		return err
	}

	tmpDst := dst + ".tmp"
	out, err := os.OpenFile(tmpDst, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0755)
	if err != nil {
		return err
	}
	defer out.Close()

	if _, err := io.Copy(out, in); err != nil {
		_ = os.Remove(tmpDst)
		return err
	}
	_ = out.Close()

	return os.Rename(tmpDst, dst)
}

func runServe(args []string) {
	fs := flag.NewFlagSet("serve", flag.ExitOnError)
	listenAddr := fs.String("listen", "127.0.0.1:9081", "Listen address (loopback only)")
	tokenPath := fs.String("token-file", "/etc/mieru-admin/token", "Path to token file")
	configPath := fs.String("config-file", "/etc/mita/server_config.json", "Path to server config file")
	_ = fs.Parse(args)

	cfgMgr := domain.NewConfigManager(*configPath)
	mitaCtrl := service.NewSystemMitaController()
	installer := service.NewInstallerService(mitaCtrl)
	logsSvc := service.NewLogsService()
	adminSvc := service.NewAdminService(cfgMgr, mitaCtrl, installer, logsSvc, Version, Commit)

	// Ensure token exists with strict permissions
	_, err := apihttp.GenerateOrReadToken(*tokenPath)
	if err != nil {
		log.Fatalf("Token error: %v", err)
	}

	srv := apihttp.NewServer(apihttp.ServerConfig{
		Addr:      *listenAddr,
		TokenPath: *tokenPath,
	}, adminSvc)

	// Graceful shutdown
	sigChan := make(chan os.Signal, 1)
	signal.Notify(sigChan, os.Interrupt, syscall.SIGTERM)

	go func() {
		<-sigChan
		log.Println("Shutting down management daemon...")
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = srv.Shutdown(ctx)
	}()

	log.Printf("Starting Mieru Admin Management Daemon on %s", *listenAddr)
	if err := srv.Start(); err != nil && err != http.ErrServerClosed {
		log.Fatalf("Server failed: %v", err)
	}
}

func runTUI(args []string) {
	fs := flag.NewFlagSet("tui", flag.ExitOnError)
	apiURL := fs.String("api-url", "http://127.0.0.1:9081/v1", "Mieru Admin API URL")
	tokenPath := fs.String("token-file", "/etc/mieru-admin/token", "Path to token file")
	_ = fs.Parse(args)

	token := os.Getenv("MIERU_ADMIN_TOKEN")
	if token == "" {
		if data, err := os.ReadFile(*tokenPath); err == nil {
			token = strings.TrimSpace(string(data))
		}
	}

	if token == "" {
		fmt.Fprintf(os.Stderr, "Error: unable to read authentication token from %s. Run under sudo/root or set MIERU_ADMIN_TOKEN.\n", *tokenPath)
		os.Exit(1)
	}

	client := apihttp.NewClient(*apiURL, token)
	model := tui.NewModel(client)

	p := tea.NewProgram(model, tea.WithAltScreen())
	if _, err := p.Run(); err != nil {
		fmt.Fprintf(os.Stderr, "TUI error: %v\n", err)
		os.Exit(1)
	}
}
