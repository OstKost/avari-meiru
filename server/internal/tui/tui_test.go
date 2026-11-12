package tui

import (
	"strings"
	"testing"

	"github.com/charmbracelet/bubbles/textinput"
	tea "github.com/charmbracelet/bubbletea"
	apihttp "mieru-admin/internal/transport/http"
)

func TestTUI_ResponsiveLayout80x24(t *testing.T) {
	client := apihttp.NewClient("http://127.0.0.1:9081/v1", "dummy")
	model := NewModel(client)

	// Test 80x24 resize
	resizeMsg := tea.WindowSizeMsg{Width: 80, Height: 24}
	updated, _ := model.Update(resizeMsg)
	m := updated.(Model)

	if m.width != 80 || m.height != 24 {
		t.Errorf("expected 80x24 size, got %dx%d", m.width, m.height)
	}

	viewStr := m.View()
	if !strings.Contains(viewStr, "Mieru Admin") {
		t.Errorf("expected title in view: %s", viewStr)
	}
	if !strings.Contains(viewStr, "Меню управления") {
		t.Errorf("expected menu header in view: %s", viewStr)
	}
}

func TestTUI_PasswordMasking(t *testing.T) {
	client := apihttp.NewClient("http://127.0.0.1:9081/v1", "dummy")
	model := NewModel(client)

	// Start wizard
	updated, _ := model.startWizard()
	m := updated.(Model)

	// Check wizard password input
	if m.input3.EchoMode != textinput.EchoPassword {
		t.Errorf("expected EchoPassword for password input in wizard, got %v", m.input3.EchoMode)
	}

	// Start export screen
	updated, _ = m.startExportScreen()
	m = updated.(Model)
	if m.input2.EchoMode != textinput.EchoPassword {
		t.Errorf("expected EchoPassword for password input in export screen, got %v", m.input2.EchoMode)
	}
}

func TestTUI_NavigationAndEscCtrlC(t *testing.T) {
	client := apihttp.NewClient("http://127.0.0.1:9081/v1", "dummy")
	model := NewModel(client)

	// Press 5 (Service screen)
	updated, _ := model.Update(tea.KeyMsg{Type: tea.KeyRunes, Runes: []rune{'5'}})
	m := updated.(Model)
	if m.screen != ScreenService {
		t.Errorf("expected ScreenService, got %v", m.screen)
	}

	// Press Esc -> back to Dashboard
	updated, _ = m.Update(tea.KeyMsg{Type: tea.KeyEsc})
	m = updated.(Model)
	if m.screen != ScreenDashboard {
		t.Errorf("expected ScreenDashboard after Esc, got %v", m.screen)
	}

	// Press Ctrl+C -> tea.Quit cmd
	_, cmd := m.Update(tea.KeyMsg{Type: tea.KeyCtrlC})
	if cmd == nil {
		t.Errorf("expected Quit cmd on Ctrl+C")
	}
}
