package domain

import (
	"encoding/json"
	"fmt"
	"strings"
)

// ClientProfile strictly matches configs/profile.schema.json
type ClientProfile struct {
	Server         string `json:"server"`
	ServerPort     int    `json:"server_port"`
	Transport      string `json:"transport"`
	Username       string `json:"username"`
	Password       string `json:"password"`
	TrafficPattern string `json:"traffic_pattern,omitempty"`
}

func (p *ClientProfile) ToJSON() ([]byte, error) {
	return json.MarshalIndent(p, "", "  ")
}

// UserSummary represents a sanitized user view for API
type UserSummary struct {
	Name            string `json:"name"`
	AllowPrivateIP  bool   `json:"allowPrivateIP,omitempty"`
	AllowLoopbackIP bool   `json:"allowLoopbackIP,omitempty"`
	Quotas          []any  `json:"quotas,omitempty"`
}

// ListUsers returns list of sanitized users from raw config
func (rc *RawConfig) ListUsers() []UserSummary {
	var users []UserSummary
	if rc == nil || rc.rawMap == nil {
		return users
	}
	usersVal, ok := rc.rawMap["users"]
	if !ok {
		return users
	}
	userList, ok := usersVal.([]any)
	if !ok {
		return users
	}
	for _, uAny := range userList {
		uMap, ok := uAny.(map[string]any)
		if !ok {
			continue
		}
		name, _ := uMap["name"].(string)
		if name == "" {
			continue
		}
		summary := UserSummary{Name: name}
		if priv, ok := uMap["allowPrivateIP"].(bool); ok {
			summary.AllowPrivateIP = priv
		}
		if loop, ok := uMap["allowLoopbackIP"].(bool); ok {
			summary.AllowLoopbackIP = loop
		}
		if q, ok := uMap["quotas"].([]any); ok {
			summary.Quotas = q
		}
		users = append(users, summary)
	}
	return users
}

// FindUserRaw returns the raw map of user if found
func (rc *RawConfig) FindUserRaw(name string) (map[string]any, int) {
	if rc == nil || rc.rawMap == nil {
		return nil, -1
	}
	usersVal, ok := rc.rawMap["users"]
	if !ok {
		return nil, -1
	}
	userList, ok := usersVal.([]any)
	if !ok {
		return nil, -1
	}
	for idx, uAny := range userList {
		uMap, ok := uAny.(map[string]any)
		if !ok {
			continue
		}
		if n, ok := uMap["name"].(string); ok && strings.EqualFold(n, name) {
			return uMap, idx
		}
	}
	return nil, -1
}

// AddUser adds a new user with hashed password. Password is never saved as plaintext!
func (rc *RawConfig) AddUser(name, rawPassword string, allowPrivateIP, allowLoopbackIP *bool) error {
	name = strings.TrimSpace(name)
	if name == "" {
		return fmt.Errorf("user name cannot be empty")
	}
	if rawPassword == "" {
		return fmt.Errorf("password cannot be empty")
	}

	if _, idx := rc.FindUserRaw(name); idx != -1 {
		return fmt.Errorf("%w: user %q already exists", ErrUserAlreadyExists, name)
	}

	hashed := HashPassword(rawPassword, name)
	newUser := map[string]any{
		"name":           name,
		"hashedPassword": hashed,
	}
	if allowPrivateIP != nil {
		newUser["allowPrivateIP"] = *allowPrivateIP
	}
	if allowLoopbackIP != nil {
		newUser["allowLoopbackIP"] = *allowLoopbackIP
	}

	var userList []any
	if uVal, ok := rc.rawMap["users"]; ok {
		if list, ok := uVal.([]any); ok {
			userList = list
		}
	}
	userList = append(userList, newUser)
	rc.rawMap["users"] = userList
	return nil
}

// ResetPassword updates the user's password with newly computed hash
func (rc *RawConfig) ResetPassword(name, newPassword string) error {
	name = strings.TrimSpace(name)
	if newPassword == "" {
		return fmt.Errorf("new password cannot be empty")
	}

	uMap, idx := rc.FindUserRaw(name)
	if idx == -1 {
		return fmt.Errorf("%w: %s", ErrUserNotFound, name)
	}

	hashed := HashPassword(newPassword, name)
	uMap["hashedPassword"] = hashed
	delete(uMap, "password") // ensure no plaintext password remains
	return nil
}

// DeleteUser removes a user by name
func (rc *RawConfig) DeleteUser(name string) error {
	name = strings.TrimSpace(name)
	usersVal, ok := rc.rawMap["users"]
	if !ok {
		return fmt.Errorf("%w: %s", ErrUserNotFound, name)
	}
	userList, ok := usersVal.([]any)
	if !ok {
		return fmt.Errorf("%w: %s", ErrUserNotFound, name)
	}

	foundIdx := -1
	for idx, uAny := range userList {
		uMap, ok := uAny.(map[string]any)
		if !ok {
			continue
		}
		if n, ok := uMap["name"].(string); ok && strings.EqualFold(n, name) {
			foundIdx = idx
			break
		}
	}

	if foundIdx == -1 {
		return fmt.Errorf("%w: %s", ErrUserNotFound, name)
	}

	userList = append(userList[:foundIdx], userList[foundIdx+1:]...)
	rc.rawMap["users"] = userList
	return nil
}

// GetPrimaryTCPPort finds the primary configured TCP port
func (rc *RawConfig) GetPrimaryTCPPort() int {
	if rc == nil || rc.rawMap == nil {
		return 28443
	}
	pbVal, ok := rc.rawMap["portBindings"]
	if !ok {
		return 28443
	}
	bindings, ok := pbVal.([]any)
	if !ok {
		return 28443
	}
	for _, bAny := range bindings {
		bMap, ok := bAny.(map[string]any)
		if !ok {
			continue
		}
		protoStr, _ := bMap["protocol"].(string)
		if strings.EqualFold(protoStr, "TCP") {
			if portFloat, ok := bMap["port"].(float64); ok && portFloat > 0 {
				return int(portFloat)
			}
		}
	}
	return 28443
}

// SetPrimaryTCPPort updates or adds the primary TCP port binding, preserving any other port bindings
func (rc *RawConfig) SetPrimaryTCPPort(port int) error {
	if port < 1 || port > 65535 {
		return fmt.Errorf("port must be between 1 and 65535, got %d", port)
	}

	var bindings []any
	if pbVal, ok := rc.rawMap["portBindings"]; ok {
		if list, ok := pbVal.([]any); ok {
			bindings = list
		}
	}

	updated := false
	for idx, bAny := range bindings {
		bMap, ok := bAny.(map[string]any)
		if !ok {
			continue
		}
		protoStr, _ := bMap["protocol"].(string)
		if strings.EqualFold(protoStr, "TCP") {
			bMap["port"] = float64(port)
			bindings[idx] = bMap
			updated = true
			break
		}
	}

	if !updated {
		bindings = append(bindings, map[string]any{
			"port":     float64(port),
			"protocol": "TCP",
		})
	}

	rc.rawMap["portBindings"] = bindings
	return nil
}

// ExportProfile verifies user password against stored hash and generates ClientProfile
func (rc *RawConfig) ExportProfile(username, password, serverHost string, portOverride int) (*ClientProfile, error) {
	username = strings.TrimSpace(username)
	if username == "" {
		return nil, fmt.Errorf("username is required")
	}
	if password == "" {
		return nil, fmt.Errorf("password is required")
	}

	uMap, idx := rc.FindUserRaw(username)
	if idx == -1 {
		return nil, fmt.Errorf("%w: %s", ErrUserNotFound, username)
	}

	storedHash, _ := uMap["hashedPassword"].(string)
	if storedHash == "" {
		// Fallback check if plaintext password was present (should not be, but for safety)
		if pt, ok := uMap["password"].(string); ok && pt != "" {
			storedHash = HashPassword(pt, username)
		}
	}

	if !VerifyPassword(password, username, storedHash) {
		return nil, fmt.Errorf("%w: incorrect password for user %s", ErrInvalidPassword, username)
	}

	port := rc.GetPrimaryTCPPort()
	if portOverride > 0 && portOverride <= 65535 {
		port = portOverride
	}

	if serverHost == "" {
		if ip, ok := rc.rawMap["listenIPAddress"].(string); ok && ip != "" && ip != "0.0.0.0" {
			serverHost = ip
		} else {
			serverHost = "127.0.0.1"
		}
	}

	profile := &ClientProfile{
		Server:     serverHost,
		ServerPort: port,
		Transport:  "TCP",
		Username:   username,
		Password:   password,
	}

	// If trafficPattern is set in config as base64 string, copy it
	if tpVal, ok := rc.rawMap["trafficPattern"]; ok {
		if tpMap, ok := tpVal.(map[string]any); ok {
			if b64, ok := tpMap["trafficPattern"].(string); ok && b64 != "" {
				profile.TrafficPattern = b64
			}
		} else if tpStr, ok := tpVal.(string); ok && tpStr != "" {
			profile.TrafficPattern = tpStr
		}
	}

	return profile, nil
}
