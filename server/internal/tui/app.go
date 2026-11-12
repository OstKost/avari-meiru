package tui

import (
	"context"
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/charmbracelet/bubbles/textinput"
	tea "github.com/charmbracelet/bubbletea"
	"mieru-admin/internal/domain"
	apihttp "mieru-admin/internal/transport/http"
	"mieru-admin/internal/service"
)

type Screen int

const (
	ScreenDashboard Screen = iota
	ScreenWizard
	ScreenPort
	ScreenUsers
	ScreenAddUser
	ScreenResetPass
	ScreenDeleteUser
	ScreenService
	ScreenLogs
	ScreenExport
)

type statusMsg struct {
	status *service.StatusResult
	err    error
}

type configMsg struct {
	config *service.ConfigResult
	err    error
}

type usersMsg struct {
	users []domain.UserSummary
	err   error
}

type previewMsg struct {
	preview *service.PreviewResult
	err     error
}

type actionResultMsg struct {
	message string
	err     error
}

type logsMsg struct {
	lines []string
	err   error
}

type Model struct {
	client        *apihttp.Client
	screen        Screen
	previousScreen Screen
	width         int
	height        int

	menuIndex     int
	menuItems     []string

	status        *service.StatusResult
	config        *service.ConfigResult
	users         []domain.UserSummary
	selectedUser  int
	logs          []string

	// Inputs
	input1        textinput.Model
	input2        textinput.Model
	input3        textinput.Model
	confirmInput  textinput.Model
	activeInput   int

	// Wizard state
	wizardStep    int
	wizardIP      string
	wizardPort    int
	wizardUser    string
	wizardPass    string

	// Preview / Diff state
	previewData   *service.PreviewResult

	infoMessage   string
	errorMessage  string
	isLoading     bool
}

func NewModel(client *apihttp.Client) Model {
	t1 := textinput.New()
	t1.Focus()

	t2 := textinput.New()
	t3 := textinput.New()

	conf := textinput.New()
	conf.Placeholder = "y/N"

	m := Model{
		client:       client,
		screen:       ScreenDashboard,
		menuItems: []string{
			"1. Обзор и состояние (Dashboard)",
			"2. Мастер первичной настройки (Setup Wizard)",
			"3. Управление портом TCP (Port Management)",
			"4. Управление пользователями (User Management)",
			"5. Управление службой mita (Service Control)",
			"6. Журнал событий (Logs)",
			"7. Экспорт профиля клиента (Export Profile)",
			"q. Выход (Exit)",
		},
		input1:       t1,
		input2:       t2,
		input3:       t3,
		confirmInput: conf,
	}
	return m
}

func (m Model) Init() tea.Cmd {
	return tea.Batch(
		m.fetchStatusCmd(),
		m.fetchConfigCmd(),
		m.fetchUsersCmd(),
	)
}

func (m Model) fetchStatusCmd() tea.Cmd {
	return func() tea.Msg {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		status, err := m.client.GetStatus(ctx)
		return statusMsg{status: status, err: err}
	}
}

func (m Model) fetchConfigCmd() tea.Cmd {
	return func() tea.Msg {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		cfg, err := m.client.GetConfig(ctx)
		return configMsg{config: cfg, err: err}
	}
}

func (m Model) fetchUsersCmd() tea.Cmd {
	return func() tea.Msg {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		users, err := m.client.ListUsers(ctx)
		return usersMsg{users: users, err: err}
	}
}

func (m Model) fetchLogsCmd() tea.Cmd {
	return func() tea.Msg {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		lines, err := m.client.GetLogs(ctx, 100)
		return logsMsg{lines: lines, err: err}
	}
}

func (m Model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmds []tea.Cmd

	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.width = msg.Width
		m.height = msg.Height
		return m, nil

	case statusMsg:
		m.isLoading = false
		if msg.err != nil {
			m.errorMessage = fmt.Sprintf("Ошибка получения статуса: %v", msg.err)
		} else {
			m.status = msg.status
		}
		return m, nil

	case configMsg:
		m.isLoading = false
		if msg.err != nil {
			m.errorMessage = fmt.Sprintf("Ошибка получения конфигурации: %v", msg.err)
		} else {
			m.config = msg.config
		}
		return m, nil

	case usersMsg:
		m.isLoading = false
		if msg.err != nil {
			m.errorMessage = fmt.Sprintf("Ошибка получения пользователей: %v", msg.err)
		} else {
			m.users = msg.users
		}
		return m, nil

	case logsMsg:
		m.isLoading = false
		if msg.err != nil {
			m.errorMessage = fmt.Sprintf("Ошибка получения логов: %v", msg.err)
		} else {
			m.logs = msg.lines
		}
		return m, nil

	case previewMsg:
		m.isLoading = false
		if msg.err != nil {
			m.errorMessage = fmt.Sprintf("Ошибка предпросмотра: %v", msg.err)
		} else {
			m.previewData = msg.preview
		}
		return m, nil

	case actionResultMsg:
		m.isLoading = false
		if msg.err != nil {
			m.errorMessage = fmt.Sprintf("Ошибка выполнения: %v", msg.err)
		} else {
			m.infoMessage = msg.message
			m.errorMessage = ""
			// Refresh
			cmds = append(cmds, m.fetchStatusCmd(), m.fetchConfigCmd(), m.fetchUsersCmd())
		}
		return m, tea.Batch(cmds...)

	case tea.KeyMsg:
		// Global exit on Ctrl+C
		if msg.Type == tea.KeyCtrlC {
			return m, tea.Quit
		}

		// Handle screens
		switch m.screen {
		case ScreenDashboard:
			return m.handleDashboardKeys(msg)
		case ScreenWizard:
			return m.handleWizardKeys(msg)
		case ScreenPort:
			return m.handlePortKeys(msg)
		case ScreenUsers:
			return m.handleUsersKeys(msg)
		case ScreenAddUser:
			return m.handleAddUserKeys(msg)
		case ScreenResetPass:
			return m.handleResetPassKeys(msg)
		case ScreenDeleteUser:
			return m.handleDeleteUserKeys(msg)
		case ScreenService:
			return m.handleServiceKeys(msg)
		case ScreenLogs:
			return m.handleLogsKeys(msg)
		case ScreenExport:
			return m.handleExportKeys(msg)
		}
	}

	return m, nil
}

func (m Model) handleDashboardKeys(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.String() {
	case "q", "Q", "esc":
		return m, tea.Quit
	case "up", "k":
		if m.menuIndex > 0 {
			m.menuIndex--
		}
	case "down", "j":
		if m.menuIndex < len(m.menuItems)-1 {
			m.menuIndex++
		}
	case "1":
		m.menuIndex = 0
		return m, tea.Batch(m.fetchStatusCmd(), m.fetchConfigCmd(), m.fetchUsersCmd())
	case "2":
		return m.startWizard()
	case "3":
		return m.startPortScreen()
	case "4":
		return m.startUsersScreen()
	case "5":
		m.screen = ScreenService
		return m, m.fetchStatusCmd()
	case "6":
		m.screen = ScreenLogs
		return m, m.fetchLogsCmd()
	case "7":
		return m.startExportScreen()
	case "enter":
		switch m.menuIndex {
		case 0: // Refresh
			return m, tea.Batch(m.fetchStatusCmd(), m.fetchConfigCmd(), m.fetchUsersCmd())
		case 1:
			return m.startWizard()
		case 2:
			return m.startPortScreen()
		case 3:
			return m.startUsersScreen()
		case 4:
			m.screen = ScreenService
			return m, m.fetchStatusCmd()
		case 5:
			m.screen = ScreenLogs
			return m, m.fetchLogsCmd()
		case 6:
			return m.startExportScreen()
		case 7:
			return m, tea.Quit
		}
	}
	return m, nil
}

func (m Model) startWizard() (tea.Model, tea.Cmd) {
	m.screen = ScreenWizard
	m.wizardStep = 1
	m.wizardIP = ""
	if m.status != nil && m.status.OS.PublicIP != "" {
		m.wizardIP = m.status.OS.PublicIP
	} else {
		m.wizardIP = "127.0.0.1"
	}
	m.wizardPort = 28443

	m.input1.Reset()
	m.input1.SetValue(m.wizardIP)
	m.input1.Placeholder = "Внешний IP / домен"
	m.input1.Focus()

	m.input2.Reset()
	m.input2.SetValue("28443")
	m.input2.Placeholder = "TCP порт"

	m.input3.Reset()
	m.input3.Placeholder = "Пароль (мин. 8 символов)"
	m.input3.EchoMode = textinput.EchoPassword

	m.infoMessage = ""
	m.errorMessage = ""
	return m, nil
}

func (m Model) handleWizardKeys(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	if msg.Type == tea.KeyEsc {
		m.screen = ScreenDashboard
		return m, nil
	}

	switch m.wizardStep {
	case 1: // IP input
		if msg.Type == tea.KeyEnter {
			val := strings.TrimSpace(m.input1.Value())
			if val == "" {
				m.errorMessage = "IP-адрес или домен не может быть пустым"
				return m, nil
			}
			m.wizardIP = val
			m.wizardStep = 2
			m.input2.Focus()
			m.errorMessage = ""
			return m, nil
		}
		var cmd tea.Cmd
		m.input1, cmd = m.input1.Update(msg)
		return m, cmd

	case 2: // Port input
		if msg.Type == tea.KeyEnter {
			val := strings.TrimSpace(m.input2.Value())
			p, err := strconv.Atoi(val)
			if err != nil || p < 1 || p > 65535 {
				m.errorMessage = "Некорректный TCP-порт (от 1 до 65535)"
				return m, nil
			}
			m.wizardPort = p
			m.wizardStep = 3
			m.input1.Reset()
			m.input1.Placeholder = "Логин первого пользователя (email / имя)"
			m.input1.Focus()
			m.errorMessage = ""
			return m, nil
		}
		var cmd tea.Cmd
		m.input2, cmd = m.input2.Update(msg)
		return m, cmd

	case 3: // Username
		if msg.Type == tea.KeyEnter {
			val := strings.TrimSpace(m.input1.Value())
			if val == "" {
				m.errorMessage = "Имя пользователя не может быть пустым"
				return m, nil
			}
			m.wizardUser = val
			m.wizardStep = 4
			m.input3.Focus()
			m.errorMessage = ""
			return m, nil
		}
		var cmd tea.Cmd
		m.input1, cmd = m.input1.Update(msg)
		return m, cmd

	case 4: // Password
		if msg.Type == tea.KeyEnter {
			pass := m.input3.Value()
			if len(pass) < 6 {
				m.errorMessage = "Пароль должен содержать не менее 6 символов"
				return m, nil
			}
			m.wizardPass = pass
			m.wizardStep = 5

			// Request preview
			candMap := map[string]any{
				"portBindings": []any{map[string]any{"port": float64(m.wizardPort), "protocol": "TCP"}},
				"users": []any{
					map[string]any{
						"name":           m.wizardUser,
						"hashedPassword": domain.HashPassword(m.wizardPass, m.wizardUser),
					},
				},
				"loggingLevel": "INFO",
				"mtu":          float64(1400),
			}
			m.errorMessage = ""
			return m, func() tea.Msg {
				ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
				defer cancel()
				prev, err := m.client.PreviewConfig(ctx, candMap)
				return previewMsg{preview: prev, err: err}
			}
		}
		var cmd tea.Cmd
		m.input3, cmd = m.input3.Update(msg)
		return m, cmd

	case 5: // Preview & Apply confirmation
		if msg.Type == tea.KeyEnter {
			// Apply config
			m.isLoading = true
			m.wizardStep = 6
			candMap := map[string]any{
				"portBindings": []any{map[string]any{"port": float64(m.wizardPort), "protocol": "TCP"}},
				"users": []any{
					map[string]any{
						"name":           m.wizardUser,
						"hashedPassword": domain.HashPassword(m.wizardPass, m.wizardUser),
					},
				},
				"loggingLevel": "INFO",
				"mtu":          float64(1400),
			}
			baseRev := ""
			if m.config != nil {
				baseRev = m.config.Revision
			}
			return m, func() tea.Msg {
				ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
				defer cancel()
				res, err := m.client.ApplyConfig(ctx, baseRev, candMap)
				if err != nil {
					return actionResultMsg{err: err}
				}
				// Start mita service
				_, _ = m.client.ControlService(ctx, "start")
				return actionResultMsg{message: fmt.Sprintf("Конфигурация успешно создана! Ревизия: %s", res.Revision)}
			}
		}
	case 6: // Finished
		if msg.Type == tea.KeyEnter || msg.Type == tea.KeyEsc {
			m.screen = ScreenDashboard
			return m, nil
		}
	}

	return m, nil
}

func (m Model) startPortScreen() (tea.Model, tea.Cmd) {
	m.screen = ScreenPort
	m.input1.Reset()
	currentPort := 28443
	if m.config != nil {
		for _, bAny := range m.config.PortBindings {
			if bMap, ok := bAny.(map[string]any); ok {
				if proto, ok := bMap["protocol"].(string); ok && strings.EqualFold(proto, "TCP") {
					if pFloat, ok := bMap["port"].(float64); ok {
						currentPort = int(pFloat)
					}
				}
			}
		}
	}
	m.input1.SetValue(strconv.Itoa(currentPort))
	m.input1.Placeholder = "Новый TCP порт"
	m.input1.Focus()
	m.previewData = nil
	m.infoMessage = ""
	m.errorMessage = ""
	return m, nil
}

func (m Model) handlePortKeys(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	if msg.Type == tea.KeyEsc {
		m.screen = ScreenDashboard
		return m, nil
	}

	if m.previewData == nil {
		if msg.Type == tea.KeyEnter {
			val := strings.TrimSpace(m.input1.Value())
			p, err := strconv.Atoi(val)
			if err != nil || p < 1 || p > 65535 {
				m.errorMessage = "Некорректный номер порта (1..65535)"
				return m, nil
			}
			// Preview change
			return m, func() tea.Msg {
				ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
				defer cancel()
				patch := map[string]any{
					"portBindings": []any{map[string]any{"port": float64(p), "protocol": "TCP"}},
				}
				prev, err := m.client.PreviewConfig(ctx, patch)
				return previewMsg{preview: prev, err: err}
			}
		}
		var cmd tea.Cmd
		m.input1, cmd = m.input1.Update(msg)
		return m, cmd
	}

	// Confirm preview and apply
	if msg.Type == tea.KeyEnter {
		m.isLoading = true
		val := strings.TrimSpace(m.input1.Value())
		p, _ := strconv.Atoi(val)
		baseRev := ""
		if m.config != nil {
			baseRev = m.config.Revision
		}

		return m, func() tea.Msg {
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			patch := map[string]any{
				"portBindings": []any{map[string]any{"port": float64(p), "protocol": "TCP"}},
			}
			res, err := m.client.ApplyConfig(ctx, baseRev, patch)
			if err != nil {
				return actionResultMsg{err: err}
			}
			return actionResultMsg{message: fmt.Sprintf("Порт TCP успешно изменён на %d! Ревизия: %s", p, res.Revision)}
		}
	}

	return m, nil
}

func (m Model) startUsersScreen() (tea.Model, tea.Cmd) {
	m.screen = ScreenUsers
	m.selectedUser = 0
	m.infoMessage = ""
	m.errorMessage = ""
	return m, m.fetchUsersCmd()
}

func (m Model) handleUsersKeys(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.String() {
	case "esc":
		m.screen = ScreenDashboard
		return m, nil
	case "up", "k":
		if m.selectedUser > 0 {
			m.selectedUser--
		}
	case "down", "j":
		if m.selectedUser < len(m.users)-1 {
			m.selectedUser++
		}
	case "a", "A":
		// Add user
		m.screen = ScreenAddUser
		m.input1.Reset()
		m.input1.Placeholder = "Имя пользователя (email)"
		m.input1.Focus()
		m.input2.Reset()
		m.input2.Placeholder = "Пароль"
		m.input2.EchoMode = textinput.EchoPassword
		m.activeInput = 0
		m.errorMessage = ""
		return m, nil
	case "r", "R":
		// Reset password
		if len(m.users) == 0 {
			return m, nil
		}
		m.screen = ScreenResetPass
		m.input1.Reset()
		m.input1.Placeholder = "Новый пароль"
		m.input1.EchoMode = textinput.EchoPassword
		m.input1.Focus()
		m.errorMessage = ""
		return m, nil
	case "d", "D":
		// Delete user
		if len(m.users) == 0 {
			return m, nil
		}
		m.screen = ScreenDeleteUser
		m.confirmInput.Reset()
		m.confirmInput.Focus()
		m.errorMessage = ""
		return m, nil
	}
	return m, nil
}

func (m Model) handleAddUserKeys(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	if msg.Type == tea.KeyEsc {
		m.screen = ScreenUsers
		return m, nil
	}
	if msg.Type == tea.KeyTab {
		if m.activeInput == 0 {
			m.activeInput = 1
			m.input1.Blur()
			m.input2.Focus()
		} else {
			m.activeInput = 0
			m.input2.Blur()
			m.input1.Focus()
		}
		return m, nil
	}
	if msg.Type == tea.KeyEnter {
		name := strings.TrimSpace(m.input1.Value())
		pass := m.input2.Value()
		if name == "" || pass == "" {
			m.errorMessage = "Имя пользователя и пароль обязательны"
			return m, nil
		}
		m.isLoading = true
		m.screen = ScreenUsers
		return m, func() tea.Msg {
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			rev, err := m.client.AddUser(ctx, name, pass)
			if err != nil {
				return actionResultMsg{err: err}
			}
			return actionResultMsg{message: fmt.Sprintf("Пользователь %s успешно добавлен! Ревизия: %s", name, rev)}
		}
	}
	var cmd tea.Cmd
	if m.activeInput == 0 {
		m.input1, cmd = m.input1.Update(msg)
	} else {
		m.input2, cmd = m.input2.Update(msg)
	}
	return m, cmd
}

func (m Model) handleResetPassKeys(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	if msg.Type == tea.KeyEsc {
		m.screen = ScreenUsers
		return m, nil
	}
	if msg.Type == tea.KeyEnter {
		pass := m.input1.Value()
		if pass == "" {
			m.errorMessage = "Пароль не может быть пустым"
			return m, nil
		}
		user := m.users[m.selectedUser].Name
		m.isLoading = true
		m.screen = ScreenUsers
		return m, func() tea.Msg {
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			rev, err := m.client.ResetUserPassword(ctx, user, pass)
			if err != nil {
				return actionResultMsg{err: err}
			}
			return actionResultMsg{message: fmt.Sprintf("Пароль пользователя %s успешно изменён! Ревизия: %s", user, rev)}
		}
	}
	var cmd tea.Cmd
	m.input1, cmd = m.input1.Update(msg)
	return m, cmd
}

func (m Model) handleDeleteUserKeys(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	if msg.Type == tea.KeyEsc {
		m.screen = ScreenUsers
		return m, nil
	}
	if msg.Type == tea.KeyEnter {
		ans := strings.ToLower(strings.TrimSpace(m.confirmInput.Value()))
		if ans != "y" && ans != "yes" && ans != "д" && ans != "да" {
			m.infoMessage = "Удаление отменено"
			m.screen = ScreenUsers
			return m, nil
		}
		user := m.users[m.selectedUser].Name
		m.isLoading = true
		m.screen = ScreenUsers
		return m, func() tea.Msg {
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			rev, err := m.client.DeleteUser(ctx, user)
			if err != nil {
				return actionResultMsg{err: err}
			}
			return actionResultMsg{message: fmt.Sprintf("Пользователь %s успешно удалён! Ревизия: %s", user, rev)}
		}
	}
	var cmd tea.Cmd
	m.confirmInput, cmd = m.confirmInput.Update(msg)
	return m, cmd
}

func (m Model) handleServiceKeys(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	switch msg.String() {
	case "esc":
		m.screen = ScreenDashboard
		return m, nil
	case "1", "s": // Start
		m.isLoading = true
		return m, func() tea.Msg {
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			res, err := m.client.ControlService(ctx, "start")
			if err != nil {
				return actionResultMsg{err: err}
			}
			return actionResultMsg{message: res.Message}
		}
	case "2", "t": // Stop
		m.isLoading = true
		return m, func() tea.Msg {
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			res, err := m.client.ControlService(ctx, "stop")
			if err != nil {
				return actionResultMsg{err: err}
			}
			return actionResultMsg{message: res.Message}
		}
	case "3", "r": // Restart
		m.isLoading = true
		return m, func() tea.Msg {
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			res, err := m.client.ControlService(ctx, "restart")
			if err != nil {
				return actionResultMsg{err: err}
			}
			return actionResultMsg{message: res.Message}
		}
	}
	return m, nil
}

func (m Model) handleLogsKeys(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	if msg.Type == tea.KeyEsc || msg.String() == "q" {
		m.screen = ScreenDashboard
		return m, nil
	}
	if msg.String() == "r" {
		return m, m.fetchLogsCmd()
	}
	return m, nil
}

func (m Model) startExportScreen() (tea.Model, tea.Cmd) {
	m.screen = ScreenExport
	m.input1.Reset()
	if len(m.users) > 0 {
		m.input1.SetValue(m.users[0].Name)
	}
	m.input1.Placeholder = "Имя пользователя"
	m.input1.Focus()

	m.input2.Reset()
	m.input2.Placeholder = "Пароль пользователя (для сверки с хешем)"
	m.input2.EchoMode = textinput.EchoPassword

	m.input3.Reset()
	m.input3.SetValue("./mieru-client-profile.json")
	m.input3.Placeholder = "Путь сохранения (0600)"

	m.activeInput = 0
	m.infoMessage = ""
	m.errorMessage = ""
	return m, nil
}

func (m Model) handleExportKeys(msg tea.KeyMsg) (tea.Model, tea.Cmd) {
	if msg.Type == tea.KeyEsc {
		m.screen = ScreenDashboard
		return m, nil
	}
	if msg.Type == tea.KeyTab {
		m.activeInput = (m.activeInput + 1) % 3
		m.input1.Blur()
		m.input2.Blur()
		m.input3.Blur()
		switch m.activeInput {
		case 0:
			m.input1.Focus()
		case 1:
			m.input2.Focus()
		case 2:
			m.input3.Focus()
		}
		return m, nil
	}
	if msg.Type == tea.KeyEnter {
		user := strings.TrimSpace(m.input1.Value())
		pass := m.input2.Value()
		savePath := strings.TrimSpace(m.input3.Value())
		if user == "" || pass == "" || savePath == "" {
			m.errorMessage = "Все поля обязательны для экспорта"
			return m, nil
		}

		m.isLoading = true
		return m, func() tea.Msg {
			ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			profile, err := m.client.ExportProfile(ctx, user, pass, "", 0)
			if err != nil {
				return actionResultMsg{err: err}
			}
			// Write file with 0600 permissions without exposing password on screen
			jsonData, err := profile.ToJSON()
			if err != nil {
				return actionResultMsg{err: err}
			}
			if err := os.WriteFile(savePath, jsonData, 0600); err != nil {
				return actionResultMsg{err: fmt.Errorf("ошибка записи в файл %s: %w", savePath, err)}
			}
			return actionResultMsg{message: fmt.Sprintf("Профиль успешно сохранён в %s (права 0600)!", savePath)}
		}
	}

	var cmd tea.Cmd
	switch m.activeInput {
	case 0:
		m.input1, cmd = m.input1.Update(msg)
	case 1:
		m.input2, cmd = m.input2.Update(msg)
	case 2:
		m.input3, cmd = m.input3.Update(msg)
	}
	return m, cmd
}
