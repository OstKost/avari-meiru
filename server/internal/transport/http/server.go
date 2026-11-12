package http

import (
	"context"
	"crypto/rand"
	"crypto/subtle"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"mieru-admin/internal/service"
)

type ServerConfig struct {
	Addr       string
	TokenPath  string
	TokenValue string // Optional in-memory override for tests
}

type Server struct {
	cfg        ServerConfig
	adminSvc   *service.AdminService
	httpServer *http.Server
	mux        *http.ServeMux
	listener   net.Listener
}

func NewServer(cfg ServerConfig, adminSvc *service.AdminService) *Server {
	if cfg.Addr == "" {
		cfg.Addr = "127.0.0.1:9081"
	}
	if cfg.TokenPath == "" {
		cfg.TokenPath = "/etc/mieru-admin/token"
	}

	s := &Server{
		cfg:      cfg,
		adminSvc: adminSvc,
		mux:      http.NewServeMux(),
	}
	s.registerRoutes()
	return s
}

// GenerateOrReadToken ensures a cryptographically secure token exists with mode 0600, root ownership, and not a symlink.
func GenerateOrReadToken(tokenPath string) (string, error) {
	if tokenPath == "" {
		tokenPath = "/etc/mieru-admin/token"
	}

	fi, err := os.Lstat(tokenPath)
	if err == nil {
		// Reject symlinks
		if fi.Mode()&os.ModeSymlink != 0 {
			return "", fmt.Errorf("security violation: token file %s is a symlink", tokenPath)
		}
		// Reject non-0600 permissions
		if fi.Mode().Perm() != 0600 {
			return "", fmt.Errorf("security violation: token file %s has permissions %o (must be 0600)", tokenPath, fi.Mode().Perm())
		}
		// Check ownership if running as root
		if os.Geteuid() == 0 {
			if stat, ok := fi.Sys().(*syscall.Stat_t); ok && stat.Uid != 0 {
				return "", fmt.Errorf("security violation: token file %s is not owned by root (uid=%d)", tokenPath, stat.Uid)
			}
		}

		data, err := os.ReadFile(tokenPath)
		if err != nil {
			return "", fmt.Errorf("read token file failed: %w", err)
		}
		token := strings.TrimSpace(string(data))
		if len(token) >= 16 {
			return token, nil
		}
		return "", fmt.Errorf("token in %s is too short (min 16 chars)", tokenPath)
	}

	if !os.IsNotExist(err) {
		return "", fmt.Errorf("stat token file failed: %w", err)
	}

	// Generate 32 bytes random token
	tokenBytes := make([]byte, 32)
	if _, err := rand.Read(tokenBytes); err != nil {
		return "", fmt.Errorf("crypto rand failed: %w", err)
	}
	tokenStr := hex.EncodeToString(tokenBytes)

	dir := filepath.Dir(tokenPath)
	if err := os.MkdirAll(dir, 0700); err != nil {
		return "", fmt.Errorf("mkdir token dir failed: %w", err)
	}

	if err := os.WriteFile(tokenPath, []byte(tokenStr+"\n"), 0600); err != nil {
		return "", fmt.Errorf("write token file failed: %w", err)
	}

	return tokenStr, nil
}

func (s *Server) registerRoutes() {
	// API v1 handlers
	s.mux.HandleFunc("GET /v1/status", s.handleGetStatus)
	s.mux.HandleFunc("POST /v1/installation", s.handleInstallMita)
	s.mux.HandleFunc("GET /v1/config", s.handleGetConfig)
	s.mux.HandleFunc("PUT /v1/config", s.handleApplyConfig)
	s.mux.HandleFunc("POST /v1/config/preview", s.handlePreviewConfig)
	s.mux.HandleFunc("GET /v1/users", s.handleListUsers)
	s.mux.HandleFunc("POST /v1/users", s.handleAddUser)
	s.mux.HandleFunc("PATCH /v1/users/{name}", s.handleResetUserPassword)
	s.mux.HandleFunc("DELETE /v1/users/{name}", s.handleDeleteUser)
	s.mux.HandleFunc("POST /v1/service", s.handleServiceControl)
	s.mux.HandleFunc("GET /v1/logs", s.handleGetLogs)
	s.mux.HandleFunc("POST /v1/profiles/export", s.handleExportProfile)
}

func (s *Server) authMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Read token
		expectedToken := s.cfg.TokenValue
		if expectedToken == "" {
			if data, err := os.ReadFile(s.cfg.TokenPath); err == nil {
				expectedToken = strings.TrimSpace(string(data))
			}
		}

		if expectedToken == "" {
			writeError(w, http.StatusUnauthorized, "UNAUTHORIZED", "Token file missing or empty on server")
			return
		}

		authHeader := r.Header.Get("Authorization")
		if authHeader == "" {
			writeError(w, http.StatusUnauthorized, "UNAUTHORIZED", "Authorization header is required")
			return
		}

		parts := strings.SplitN(authHeader, " ", 2)
		if len(parts) != 2 || !strings.EqualFold(parts[0], "Bearer") {
			writeError(w, http.StatusUnauthorized, "UNAUTHORIZED", "Authorization scheme must be Bearer")
			return
		}

		providedToken := strings.TrimSpace(parts[1])
		if subtle.ConstantTimeCompare([]byte(providedToken), []byte(expectedToken)) != 1 {
			writeError(w, http.StatusUnauthorized, "UNAUTHORIZED", "Invalid authorization token")
			return
		}

		next.ServeHTTP(w, r)
	})
}

// loggingAndLimitMiddleware limits body size to 1MB and logs sanitized request metadata
func (s *Server) loggingAndLimitMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		// Limit request body
		r.Body = http.MaxBytesReader(w, r.Body, 1024*1024)

		wrapped := &responseWriterWrapper{ResponseWriter: w, statusCode: http.StatusOK}

		defer func() {
			if rec := recover(); rec != nil {
				wrapped.statusCode = http.StatusInternalServerError
				writeError(wrapped, http.StatusInternalServerError, "INTERNAL_ERROR", "Internal server error occurred")
			}
			duration := time.Since(start)
			// Log sanitized: method, path, status, duration (NO secrets, NO bodies)
			log.Printf("[API] %s %s -> %d (%v)", r.Method, r.URL.Path, wrapped.statusCode, duration)
		}()

		next.ServeHTTP(wrapped, r)
	})
}

type responseWriterWrapper struct {
	http.ResponseWriter
	statusCode int
}

func (rw *responseWriterWrapper) WriteHeader(code int) {
	rw.statusCode = code
	rw.ResponseWriter.WriteHeader(code)
}

func (s *Server) Start() error {
	host, _, err := net.SplitHostPort(s.cfg.Addr)
	if err != nil {
		return fmt.Errorf("invalid listen address: %w", err)
	}
	// Verify host is strictly loopback (127.0.0.1 or localhost)
	if host != "127.0.0.1" && host != "localhost" && host != "::1" {
		return fmt.Errorf("security violation: server must bind only to loopback (127.0.0.1), got %s", host)
	}

	ln, err := net.Listen("tcp", s.cfg.Addr)
	if err != nil {
		return fmt.Errorf("listen on %s failed: %w", s.cfg.Addr, err)
	}
	s.listener = ln

	handler := s.loggingAndLimitMiddleware(s.authMiddleware(s.mux))

	s.httpServer = &http.Server{
		Handler:      handler,
		ReadTimeout:  15 * time.Second,
		WriteTimeout: 15 * time.Second,
		IdleTimeout:  60 * time.Second,
	}

	return s.httpServer.Serve(ln)
}

func (s *Server) Shutdown(ctx context.Context) error {
	if s.httpServer != nil {
		return s.httpServer.Shutdown(ctx)
	}
	return nil
}

func (s *Server) Addr() string {
	if s.listener != nil {
		return s.listener.Addr().String()
	}
	return s.cfg.Addr
}

type ErrorResponse struct {
	Code    string `json:"code"`
	Message string `json:"message"`
	Details string `json:"details,omitempty"`
}

func writeJSON(w http.ResponseWriter, status int, data any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(data)
}

func writeError(w http.ResponseWriter, status int, code, message string) {
	writeJSON(w, status, ErrorResponse{
		Code:    code,
		Message: service.SanitizeLogLine(message),
	})
}
