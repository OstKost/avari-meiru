package tui

import (
	"fmt"
	"strings"
)

func (m Model) View() string {
	var s strings.Builder

	// Top title bar
	s.WriteString(titleStyle.Render(" Mieru Admin TUI  |  Управление сервером Mieru (mita) ") + "\n\n")

	// Info / Error banners
	if m.errorMessage != "" {
		s.WriteString(errorStyle.Render("ОШИБКА: "+m.errorMessage) + "\n\n")
	}
	if m.infoMessage != "" {
		s.WriteString(successStyle.Render("ИНФО: "+m.infoMessage) + "\n\n")
	}
	if m.isLoading {
		s.WriteString(warningStyle.Render("⏳ Выполняется запрос к API...") + "\n\n")
	}

	switch m.screen {
	case ScreenDashboard:
		s.WriteString(m.viewDashboard())
	case ScreenWizard:
		s.WriteString(m.viewWizard())
	case ScreenPort:
		s.WriteString(m.viewPort())
	case ScreenUsers:
		s.WriteString(m.viewUsers())
	case ScreenAddUser:
		s.WriteString(m.viewAddUser())
	case ScreenResetPass:
		s.WriteString(m.viewResetPass())
	case ScreenDeleteUser:
		s.WriteString(m.viewDeleteUser())
	case ScreenService:
		s.WriteString(m.viewService())
	case ScreenLogs:
		s.WriteString(m.viewLogs())
	case ScreenExport:
		s.WriteString(m.viewExport())
	}

	return s.String()
}

func (m Model) viewDashboard() string {
	var s strings.Builder

	// Status card
	var statusContent strings.Builder
	if m.status != nil {
		statusContent.WriteString(fmt.Sprintf("• ОС: %s (%s, %s)\n", m.status.OS.Name, m.status.OS.Arch, m.status.OS.Version))
		statusContent.WriteString(fmt.Sprintf("• Внешний / локальный IP: %s\n", m.status.OS.PublicIP))

		installedStr := errorStyle.Render("НЕТ")
		if m.status.Mita.Installed {
			installedStr = successStyle.Render("Установлен (" + m.status.Mita.Version + ")")
		}
		statusContent.WriteString(fmt.Sprintf("• Пакет mita: %s\n", installedStr))

		systemdStr := errorStyle.Render("inactive")
		if m.status.Mita.SystemdActive {
			systemdStr = successStyle.Render("active (running)")
		}
		statusContent.WriteString(fmt.Sprintf("• Systemd служба: %s\n", systemdStr))

		proxyStr := errorStyle.Render("STOPPED")
		if m.status.Mita.ProxyRunning {
			proxyStr = successStyle.Render("RUNNING")
		}
		statusContent.WriteString(fmt.Sprintf("• Mita proxy: %s\n", proxyStr))
		statusContent.WriteString(fmt.Sprintf("• Основной TCP порт: %d\n", m.status.Mita.Port))
		statusContent.WriteString(fmt.Sprintf("• Пользователей в системе: %d\n", m.status.Mita.UsersCount))
	} else {
		statusContent.WriteString("Загрузка статуса сервера...\n")
	}

	s.WriteString(cardStyle.Render(statusContent.String()) + "\n")

	// Menu
	s.WriteString(headerStyle.Render("Меню управления:") + "\n")
	for i, item := range m.menuItems {
		if i == m.menuIndex {
			s.WriteString(activeItemStyle.Render("▸ " + item) + "\n")
		} else {
			s.WriteString(inactiveItemStyle.Render("  " + item) + "\n")
		}
	}

	s.WriteString(helpStyle.Render("↑/↓: выбор пункта  •  Enter: открыть  •  1..7: быстрый выбор  •  q/Esc: выход") + "\n")
	return s.String()
}

func (m Model) viewWizard() string {
	var s strings.Builder
	s.WriteString(headerStyle.Render("Мастер первичной настройки Mieru") + "\n")

	switch m.wizardStep {
	case 1:
		s.WriteString("Шаг 1 из 5: Внешний IP или домен сервера\n")
		s.WriteString(subHeaderStyle.Render("Укажите IP или имя хоста, к которому будут подключаться клиенты:") + "\n\n")
		s.WriteString(m.input1.View() + "\n\n")
		s.WriteString(helpStyle.Render("Enter: продолжить  •  Esc: назад в меню"))
	case 2:
		s.WriteString("Шаг 2 из 5: TCP Порт\n")
		s.WriteString(subHeaderStyle.Render("Укажите входящий TCP порт прокси (по умолчанию 28443):") + "\n\n")
		s.WriteString(m.input2.View() + "\n\n")
		s.WriteString(helpStyle.Render("Enter: продолжить  •  Esc: назад в меню"))
	case 3:
		s.WriteString("Шаг 3 из 5: Первый пользователь\n")
		s.WriteString(subHeaderStyle.Render("Логин (обычно email):") + "\n\n")
		s.WriteString(m.input1.View() + "\n\n")
		s.WriteString(helpStyle.Render("Enter: продолжить  •  Esc: назад в меню"))
	case 4:
		s.WriteString("Шаг 4 из 5: Пароль пользователя\n")
		s.WriteString(subHeaderStyle.Render("Пароль маскируется. Сервер сохранит только криптографический хеш:") + "\n\n")
		s.WriteString(m.input3.View() + "\n\n")
		s.WriteString(helpStyle.Render("Enter: сформировать конфигурацию  •  Esc: отмена"))
	case 5:
		s.WriteString("Шаг 5 из 5: Предпросмотр изменений и подтверждение\n")
		if m.previewData != nil {
			s.WriteString(cardStyle.Render(m.previewData.Diff) + "\n")
		}
		s.WriteString(successStyle.Render("Нажмите Enter для применения конфигурации и запуска службы.") + "\n")
		s.WriteString(helpStyle.Render("Enter: применить  •  Esc: отмена"))
	case 6:
		s.WriteString(successStyle.Render("✓ Настройка успешно завершена!") + "\n\n")
		s.WriteString(fmt.Sprintf("• Сервер: %s\n", m.wizardIP))
		s.WriteString(fmt.Sprintf("• Порт: %d (TCP)\n", m.wizardPort))
		s.WriteString(fmt.Sprintf("• Пользователь: %s\n\n", m.wizardUser))
		s.WriteString(helpStyle.Render("Нажмите Enter или Esc для возврата в главное меню."))
	}

	return s.String()
}

func (m Model) viewPort() string {
	var s strings.Builder
	s.WriteString(headerStyle.Render("Управление TCP-портом") + "\n")

	if m.previewData == nil {
		s.WriteString("Введите новый номер TCP-порта (1..65535):\n\n")
		s.WriteString(m.input1.View() + "\n\n")
		s.WriteString(helpStyle.Render("Enter: предпросмотр изменений  •  Esc: отмена"))
	} else {
		s.WriteString("Предпросмотр изменений конфигурации:\n")
		s.WriteString(cardStyle.Render(m.previewData.Diff) + "\n")
		s.WriteString(warningStyle.Render("Применить изменения и перезапустить mita?") + "\n\n")
		s.WriteString(helpStyle.Render("Enter: применить  •  Esc: отмена"))
	}
	return s.String()
}

func (m Model) viewUsers() string {
	var s strings.Builder
	s.WriteString(headerStyle.Render("Управление пользователями") + "\n")

	if len(m.users) == 0 {
		s.WriteString("Пользователи пока не созданы.\n\n")
	} else {
		for i, u := range m.users {
			if i == m.selectedUser {
				s.WriteString(activeItemStyle.Render(fmt.Sprintf("▸ [%d] %s", i+1, u.Name)) + "\n")
			} else {
				s.WriteString(inactiveItemStyle.Render(fmt.Sprintf("  [%d] %s", i+1, u.Name)) + "\n")
			}
		}
		s.WriteString("\n")
	}

	s.WriteString(helpStyle.Render("a: Добавить пользователя  •  r: Сменить пароль  •  d: Удалить пользователя  •  Esc: Назад"))
	return s.String()
}

func (m Model) viewAddUser() string {
	var s strings.Builder
	s.WriteString(headerStyle.Render("Добавление пользователя") + "\n")
	s.WriteString("Логин (email):\n")
	s.WriteString(m.input1.View() + "\n\n")
	s.WriteString("Пароль (маскируется):\n")
	s.WriteString(m.input2.View() + "\n\n")
	s.WriteString(helpStyle.Render("Tab: переключение полей  •  Enter: сохранить  •  Esc: отмена"))
	return s.String()
}

func (m Model) viewResetPass() string {
	var s strings.Builder
	user := ""
	if len(m.users) > m.selectedUser {
		user = m.users[m.selectedUser].Name
	}
	s.WriteString(headerStyle.Render(fmt.Sprintf("Смена пароля пользователя %s", user)) + "\n")
	s.WriteString("Новый пароль:\n")
	s.WriteString(m.input1.View() + "\n\n")
	s.WriteString(helpStyle.Render("Enter: сохранить пароль  •  Esc: отмена"))
	return s.String()
}

func (m Model) viewDeleteUser() string {
	var s strings.Builder
	user := ""
	if len(m.users) > m.selectedUser {
		user = m.users[m.selectedUser].Name
	}
	s.WriteString(headerStyle.Render(fmt.Sprintf("Удаление пользователя %s", user)) + "\n")
	s.WriteString(warningStyle.Render(fmt.Sprintf("Вы уверены, что хотите удалить пользователя %s? (y/N)", user)) + "\n\n")
	s.WriteString(m.confirmInput.View() + "\n\n")
	s.WriteString(helpStyle.Render("Enter: подтвердить  •  Esc: отмена"))
	return s.String()
}

func (m Model) viewService() string {
	var s strings.Builder
	s.WriteString(headerStyle.Render("Управление службой mita") + "\n")

	if m.status != nil {
		sysStr := "inactive"
		if m.status.Mita.SystemdActive {
			sysStr = "active (running)"
		}
		proxyStr := "STOPPED"
		if m.status.Mita.ProxyRunning {
			proxyStr = "RUNNING"
		}
		s.WriteString(fmt.Sprintf("• Systemd статус: %s\n", sysStr))
		s.WriteString(fmt.Sprintf("• Proxy статус: %s\n\n", proxyStr))
	}

	s.WriteString("[1 / s] Запустить службу (Start)\n")
	s.WriteString("[2 / t] Остановить службу (Stop)\n")
	s.WriteString("[3 / r] Перезапустить службу (Restart)\n\n")
	s.WriteString(helpStyle.Render("Esc: назад в меню"))
	return s.String()
}

func (m Model) viewLogs() string {
	var s strings.Builder
	s.WriteString(headerStyle.Render("Журнал событий (очищенный от секретов)") + "\n")

	if len(m.logs) == 0 {
		s.WriteString("Логов пока нет.\n")
	} else {
		maxLines := 15
		start := 0
		if len(m.logs) > maxLines {
			start = len(m.logs) - maxLines
		}
		for _, l := range m.logs[start:] {
			s.WriteString(" " + l + "\n")
		}
	}
	s.WriteString("\n" + helpStyle.Render("r: обновить логи  •  q/Esc: назад в меню"))
	return s.String()
}

func (m Model) viewExport() string {
	var s strings.Builder
	s.WriteString(headerStyle.Render("Экспорт клиентского профиля (JSON)") + "\n")
	s.WriteString(subHeaderStyle.Render("Для экспорта необходимо ввести пароль пользователя для проверки хеша на сервере:") + "\n\n")

	s.WriteString("Пользователь:\n")
	s.WriteString(m.input1.View() + "\n\n")

	s.WriteString("Пароль пользователя:\n")
	s.WriteString(m.input2.View() + "\n\n")

	s.WriteString("Путь сохранения файла (права 0600):\n")
	s.WriteString(m.input3.View() + "\n\n")

	s.WriteString(helpStyle.Render("Tab: переключение полей  •  Enter: экспортировать  •  Esc: отмена"))
	return s.String()
}
