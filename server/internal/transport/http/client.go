package http

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"mieru-admin/internal/domain"
	"mieru-admin/internal/service"
)

type Client struct {
	BaseURL    string
	Token      string
	HTTPClient *http.Client
}

func NewClient(baseURL, token string) *Client {
	if baseURL == "" {
		baseURL = "http://127.0.0.1:9081/v1"
	}
	baseURL = strings.TrimSuffix(baseURL, "/")
	return &Client{
		BaseURL: baseURL,
		Token:   token,
		HTTPClient: &http.Client{
			Timeout: 30 * time.Second,
		},
	}
}

func (c *Client) doRequest(ctx context.Context, method, path string, reqBody any, respData any) error {
	var bodyReader io.Reader
	if reqBody != nil {
		b, err := json.Marshal(reqBody)
		if err != nil {
			return fmt.Errorf("marshal request: %w", err)
		}
		bodyReader = bytes.NewReader(b)
	}

	url := c.BaseURL + path
	req, err := http.NewRequestWithContext(ctx, method, url, bodyReader)
	if err != nil {
		return fmt.Errorf("create request: %w", err)
	}

	req.Header.Set("Authorization", "Bearer "+c.Token)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")

	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return fmt.Errorf("http request failed: %w", err)
	}
	defer resp.Body.Close()

	respBytes, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("read response body: %w", err)
	}

	if resp.StatusCode >= 400 {
		var errResp ErrorResponse
		if err := json.Unmarshal(respBytes, &errResp); err == nil && errResp.Message != "" {
			return fmt.Errorf("API error (%s): %s", errResp.Code, errResp.Message)
		}
		return fmt.Errorf("API returned HTTP %d: %s", resp.StatusCode, string(respBytes))
	}

	if respData != nil && len(respBytes) > 0 {
		if err := json.Unmarshal(respBytes, respData); err != nil {
			return fmt.Errorf("unmarshal response: %w", err)
		}
	}
	return nil
}

func (c *Client) GetStatus(ctx context.Context) (*service.StatusResult, error) {
	var res service.StatusResult
	err := c.doRequest(ctx, http.MethodGet, "/status", nil, &res)
	return &res, err
}

func (c *Client) InstallMita(ctx context.Context, debPath string) (string, error) {
	req := map[string]any{"deb_path": debPath}
	var res map[string]any
	err := c.doRequest(ctx, http.MethodPost, "/installation", req, &res)
	if err != nil {
		return "", err
	}
	ver, _ := res["version"].(string)
	return ver, nil
}

func (c *Client) GetConfig(ctx context.Context) (*service.ConfigResult, error) {
	var res service.ConfigResult
	err := c.doRequest(ctx, http.MethodGet, "/config", nil, &res)
	return &res, err
}

func (c *Client) PreviewConfig(ctx context.Context, patch map[string]any) (*service.PreviewResult, error) {
	req := map[string]any{"patch": patch}
	var res service.PreviewResult
	err := c.doRequest(ctx, http.MethodPost, "/config/preview", req, &res)
	return &res, err
}

func (c *Client) ApplyConfig(ctx context.Context, baseRev string, config map[string]any) (*service.ApplyResult, error) {
	req := map[string]any{"base_revision": baseRev, "config": config}
	var res service.ApplyResult
	err := c.doRequest(ctx, http.MethodPut, "/config", req, &res)
	return &res, err
}

func (c *Client) ListUsers(ctx context.Context) ([]domain.UserSummary, error) {
	var res struct {
		Users []domain.UserSummary `json:"users"`
	}
	err := c.doRequest(ctx, http.MethodGet, "/users", nil, &res)
	return res.Users, err
}

func (c *Client) AddUser(ctx context.Context, name, password string) (string, error) {
	req := map[string]any{"name": name, "password": password}
	var res struct {
		Revision string `json:"revision"`
	}
	err := c.doRequest(ctx, http.MethodPost, "/users", req, &res)
	return res.Revision, err
}

func (c *Client) ResetUserPassword(ctx context.Context, name, password string) (string, error) {
	req := map[string]any{"password": password}
	var res struct {
		Revision string `json:"revision"`
	}
	err := c.doRequest(ctx, http.MethodPatch, "/users/"+name, req, &res)
	return res.Revision, err
}

func (c *Client) DeleteUser(ctx context.Context, name string) (string, error) {
	var res struct {
		Revision string `json:"revision"`
	}
	err := c.doRequest(ctx, http.MethodDelete, "/users/"+name, nil, &res)
	return res.Revision, err
}

func (c *Client) ControlService(ctx context.Context, action string) (*service.ServiceControlResult, error) {
	req := map[string]any{"action": action}
	var res service.ServiceControlResult
	err := c.doRequest(ctx, http.MethodPost, "/service", req, &res)
	return &res, err
}

func (c *Client) GetLogs(ctx context.Context, lines int) ([]string, error) {
	var res struct {
		Lines []string `json:"lines"`
	}
	path := fmt.Sprintf("/logs?lines=%d", lines)
	err := c.doRequest(ctx, http.MethodGet, path, nil, &res)
	return res.Lines, err
}

func (c *Client) ExportProfile(ctx context.Context, username, password, serverHost string, port int) (*domain.ClientProfile, error) {
	req := map[string]any{
		"username":    username,
		"password":    password,
		"server_host": serverHost,
		"server_port": port,
	}
	var profile domain.ClientProfile
	err := c.doRequest(ctx, http.MethodPost, "/profiles/export", req, &profile)
	return &profile, err
}
