package http

import (
	"encoding/json"
	"errors"
	"net/http"
	"strconv"

	"mieru-admin/internal/domain"
	"mieru-admin/internal/service"
)

func (s *Server) handleGetStatus(w http.ResponseWriter, r *http.Request) {
	status, err := s.adminSvc.GetStatus(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, "INTERNAL_ERROR", err.Error())
		return
	}
	writeJSON(w, http.StatusOK, status)
}

func (s *Server) handleInstallMita(w http.ResponseWriter, r *http.Request) {
	var req struct {
		DebPath string `json:"deb_path"`
	}
	_ = json.NewDecoder(r.Body).Decode(&req)

	ver, err := s.adminSvc.InstallMita(r.Context(), req.DebPath)
	if err != nil {
		if errors.Is(err, service.ErrAlreadyInstalled) {
			writeJSON(w, http.StatusOK, map[string]any{
				"success": true,
				"version": ver,
				"message": "mita is already installed with expected version",
			})
			return
		}
		if errors.Is(err, service.ErrChecksumMismatch) || errors.Is(err, service.ErrUnsupportedOS) || errors.Is(err, service.ErrUnsupportedVersion) {
			writeError(w, http.StatusBadRequest, "INSTALLATION_REJECTED", err.Error())
			return
		}
		writeError(w, http.StatusInternalServerError, "INSTALLATION_FAILED", err.Error())
		return
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"success": true,
		"version": ver,
		"message": "mita package installed successfully",
	})
}

func (s *Server) handleGetConfig(w http.ResponseWriter, r *http.Request) {
	cfg, err := s.adminSvc.GetConfig(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, "LOAD_CONFIG_FAILED", err.Error())
		return
	}
	writeJSON(w, http.StatusOK, cfg)
}

func (s *Server) handlePreviewConfig(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Patch map[string]any `json:"patch"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "INVALID_JSON", "Invalid JSON payload in request body")
		return
	}
	if req.Patch == nil {
		req.Patch = make(map[string]any)
	}

	preview, err := s.adminSvc.PreviewConfig(r.Context(), req.Patch)
	if err != nil {
		writeError(w, http.StatusBadRequest, "PREVIEW_FAILED", err.Error())
		return
	}
	writeJSON(w, http.StatusOK, preview)
}

func (s *Server) handleApplyConfig(w http.ResponseWriter, r *http.Request) {
	var req struct {
		BaseRevision string         `json:"base_revision"`
		Config       map[string]any `json:"config"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "INVALID_JSON", "Invalid JSON payload in request body")
		return
	}
	if req.Config == nil {
		writeError(w, http.StatusBadRequest, "MISSING_CONFIG", "config field is required")
		return
	}

	res, err := s.adminSvc.ApplyConfig(r.Context(), req.BaseRevision, req.Config)
	if err != nil {
		if errors.Is(err, domain.ErrConflict) {
			writeError(w, http.StatusConflict, "CONFLICT", err.Error())
			return
		}
		if errors.Is(err, domain.ErrValidationFailed) {
			writeError(w, http.StatusBadRequest, "VALIDATION_FAILED", err.Error())
			return
		}
		writeError(w, http.StatusInternalServerError, "APPLY_FAILED", err.Error())
		return
	}

	writeJSON(w, http.StatusOK, res)
}

func (s *Server) handleListUsers(w http.ResponseWriter, r *http.Request) {
	users, err := s.adminSvc.ListUsers(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, "INTERNAL_ERROR", err.Error())
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"users": users,
	})
}

func (s *Server) handleAddUser(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Name            string `json:"name"`
		Password        string `json:"password"`
		AllowPrivateIP  *bool  `json:"allowPrivateIP"`
		AllowLoopbackIP *bool  `json:"allowLoopbackIP"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "INVALID_JSON", "Invalid request body")
		return
	}
	if req.Name == "" || req.Password == "" {
		writeError(w, http.StatusBadRequest, "INVALID_ARGUMENT", "name and password are required")
		return
	}

	rev, err := s.adminSvc.AddUser(r.Context(), req.Name, req.Password, req.AllowPrivateIP, req.AllowLoopbackIP)
	if err != nil {
		if errors.Is(err, domain.ErrUserAlreadyExists) {
			writeError(w, http.StatusConflict, "USER_EXISTS", err.Error())
			return
		}
		if errors.Is(err, domain.ErrValidationFailed) {
			writeError(w, http.StatusBadRequest, "VALIDATION_FAILED", err.Error())
			return
		}
		writeError(w, http.StatusInternalServerError, "ADD_USER_FAILED", err.Error())
		return
	}

	writeJSON(w, http.StatusCreated, map[string]any{
		"success":  true,
		"name":     req.Name,
		"revision": rev,
	})
}

func (s *Server) handleResetUserPassword(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	if name == "" {
		writeError(w, http.StatusBadRequest, "INVALID_ARGUMENT", "username path parameter is required")
		return
	}

	var req struct {
		Password string `json:"password"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.Password == "" {
		writeError(w, http.StatusBadRequest, "INVALID_ARGUMENT", "password is required")
		return
	}

	rev, err := s.adminSvc.ResetUserPassword(r.Context(), name, req.Password)
	if err != nil {
		if errors.Is(err, domain.ErrUserNotFound) {
			writeError(w, http.StatusNotFound, "NOT_FOUND", err.Error())
			return
		}
		writeError(w, http.StatusInternalServerError, "RESET_PASSWORD_FAILED", err.Error())
		return
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"success":  true,
		"name":     name,
		"revision": rev,
	})
}

func (s *Server) handleDeleteUser(w http.ResponseWriter, r *http.Request) {
	name := r.PathValue("name")
	if name == "" {
		writeError(w, http.StatusBadRequest, "INVALID_ARGUMENT", "username path parameter is required")
		return
	}

	rev, err := s.adminSvc.DeleteUser(r.Context(), name)
	if err != nil {
		if errors.Is(err, domain.ErrUserNotFound) {
			writeError(w, http.StatusNotFound, "NOT_FOUND", err.Error())
			return
		}
		writeError(w, http.StatusInternalServerError, "DELETE_USER_FAILED", err.Error())
		return
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"success":  true,
		"name":     name,
		"revision": rev,
	})
}

func (s *Server) handleServiceControl(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Action string `json:"action"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.Action == "" {
		writeError(w, http.StatusBadRequest, "INVALID_ARGUMENT", "action is required (start, stop, restart)")
		return
	}

	res, err := s.adminSvc.ControlService(r.Context(), req.Action)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "SERVICE_ACTION_FAILED", err.Error())
		return
	}

	writeJSON(w, http.StatusOK, res)
}

func (s *Server) handleGetLogs(w http.ResponseWriter, r *http.Request) {
	lines := 100
	if lStr := r.URL.Query().Get("lines"); lStr != "" {
		if n, err := strconv.Atoi(lStr); err == nil && n > 0 {
			lines = n
		}
	}

	logs, err := s.adminSvc.GetLogs(r.Context(), lines)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "LOGS_FAILED", err.Error())
		return
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"lines": logs,
	})
}

func (s *Server) handleExportProfile(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Username   string `json:"username"`
		Password   string `json:"password"`
		ServerHost string `json:"server_host"`
		ServerPort int    `json:"server_port"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "INVALID_JSON", "Invalid request body")
		return
	}
	if req.Username == "" || req.Password == "" {
		writeError(w, http.StatusBadRequest, "INVALID_ARGUMENT", "username and password are required")
		return
	}

	profile, err := s.adminSvc.ExportProfile(r.Context(), req.Username, req.Password, req.ServerHost, req.ServerPort)
	if err != nil {
		if errors.Is(err, domain.ErrInvalidPassword) {
			writeError(w, http.StatusUnauthorized, "INVALID_CREDENTIALS", "Введённый пароль не соответствует сохранённому хешу")
			return
		}
		if errors.Is(err, domain.ErrUserNotFound) {
			writeError(w, http.StatusNotFound, "NOT_FOUND", "Пользователь не найден")
			return
		}
		writeError(w, http.StatusBadRequest, "EXPORT_FAILED", err.Error())
		return
	}

	writeJSON(w, http.StatusOK, profile)
}
