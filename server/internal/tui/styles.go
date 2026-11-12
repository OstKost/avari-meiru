package tui

import "github.com/charmbracelet/lipgloss"

var (
	titleStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#FAFAFA")).
			Background(lipgloss.Color("#7D56F4")).
			Padding(0, 1)

	headerStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#7D56F4")).
			MarginBottom(1)

	subHeaderStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#9E9E9E")).
			MarginBottom(1)

	cardStyle = lipgloss.NewStyle().
			Border(lipgloss.RoundedBorder()).
			BorderForeground(lipgloss.Color("#7D56F4")).
			Padding(1, 2).
			MarginBottom(1)

	successStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#04B575"))

	warningStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#FFB86C"))

	errorStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#FF5555"))

	activeItemStyle = lipgloss.NewStyle().
			Bold(true).
			Foreground(lipgloss.Color("#50FA7B")).
			PaddingLeft(2)

	inactiveItemStyle = lipgloss.NewStyle().
				Foreground(lipgloss.Color("#F8F8F2")).
				PaddingLeft(2)

	cursorStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#BD93F9")).
			Bold(true)

	helpStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#6272A4")).
			MarginTop(1)

	diffAddStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#50FA7B"))

	diffRemoveStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("#FF5555"))
)
