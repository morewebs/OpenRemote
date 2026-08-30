package telegram

import (
	"context"
	"fmt"
	"log"
	"strings"
	"sync"

	tgbot "github.com/go-telegram/bot"
	"github.com/go-telegram/bot/models"
	"github.com/morewebs/OpenRemote/internal/core/approval"
	"github.com/morewebs/OpenRemote/internal/core/chat"
	"github.com/morewebs/OpenRemote/internal/core/events"
	"github.com/morewebs/OpenRemote/internal/protocol"
)

type Config struct {
	Token          string
	AllowedUserIDs []int64
}

type Bot struct {
	mu           sync.RWMutex
	cfg          Config
	bot          *tgbot.Bot
	bus          *events.Bus
	approvals    *approval.Registry
	running      bool
	lastError    string
	sessionChats map[string]int64 // sessionID -> telegram chatID
}

type Status struct {
	Enabled   bool   `json:"enabled"`
	Running   bool   `json:"running"`
	Username  string `json:"username,omitempty"`
	LastError string `json:"lastError,omitempty"`
}

func New(cfg Config, bus *events.Bus, approvals *approval.Registry) *Bot {
	return &Bot{
		cfg:          cfg,
		bus:          bus,
		approvals:    approvals,
		sessionChats: make(map[string]int64),
	}
}

func (b *Bot) Start(ctx context.Context) error {
	if b.cfg.Token == "" {
		return nil
	}

	opts := []tgbot.Option{
		tgbot.WithDefaultHandler(b.handleUpdate),
		tgbot.WithCallbackQueryDataHandler("apr_", tgbot.MatchTypePrefix, b.handleApprovalCallback),
	}

	botInstance, err := tgbot.New(b.cfg.Token, opts...)
	if err != nil {
		b.mu.Lock()
		b.lastError = err.Error()
		b.mu.Unlock()
		return fmt.Errorf("failed to init telegram bot: %w", err)
	}

	b.mu.Lock()
	b.bot = botInstance
	b.running = true
	b.mu.Unlock()

	log.Printf("[telegram] bot initialized successfully")
	go botInstance.Start(ctx)
	return nil
}

func (b *Bot) isUserAllowed(userID int64) bool {
	if len(b.cfg.AllowedUserIDs) == 0 {
		return true
	}
	for _, id := range b.cfg.AllowedUserIDs {
		if id == userID {
			return true
		}
	}
	return false
}

func (b *Bot) handleUpdate(ctx context.Context, bot *tgbot.Bot, update *models.Update) {
	if update.Message == nil || update.Message.From == nil {
		return
	}
	if !b.isUserAllowed(update.Message.From.ID) {
		_, _ = bot.SendMessage(ctx, &tgbot.SendMessageParams{
			ChatID: update.Message.Chat.ID,
			Text:   "⛔ Unauthorized: Your Telegram User ID is not allowed to control this OpenRemote instance.",
		})
		return
	}

	text := strings.TrimSpace(update.Message.Text)
	switch {
	case text == "/start":
		msg := "🤖 *OpenRemote Telegram Gateway*\n\n" +
			"You are connected to the OpenRemote companion daemon.\n" +
			"• Approvals and alerts will appear here with interactive buttons.\n" +
			"• Use `/sessions` to list active sessions.\n" +
			"• Use `/health` for daemon status."
		_, _ = bot.SendMessage(ctx, &tgbot.SendMessageParams{
			ChatID:    update.Message.Chat.ID,
			Text:      msg,
			ParseMode: models.ParseModeMarkdown,
		})
	case text == "/health":
		_, _ = bot.SendMessage(ctx, &tgbot.SendMessageParams{
			ChatID: update.Message.Chat.ID,
			Text:   "✅ OpenRemote Daemon is healthy and active.",
		})
	case text == "/sessions":
		if b.bus != nil {
			list, _ := b.bus.ListSessions()
			if len(list) == 0 {
				_, _ = bot.SendMessage(ctx, &tgbot.SendMessageParams{
					ChatID: update.Message.Chat.ID,
					Text:   "No active sessions found.",
				})
				return
			}
			var sb strings.Builder
			sb.WriteString("📋 *Active Sessions:*\n")
			for _, s := range list {
				sb.WriteString(fmt.Sprintf("• `%v` (%v) — %v\n", s["sessionId"], s["agentId"], s["status"]))
			}
			_, _ = bot.SendMessage(ctx, &tgbot.SendMessageParams{
				ChatID:    update.Message.Chat.ID,
				Text:      sb.String(),
				ParseMode: models.ParseModeMarkdown,
			})
		}
	}
}

func (b *Bot) handleApprovalCallback(ctx context.Context, bot *tgbot.Bot, update *models.Update) {
	if update.CallbackQuery == nil {
		return
	}
	data := update.CallbackQuery.Data
	parts := strings.Split(data, "_")
	if len(parts) < 3 {
		return
	}
	approved := parts[1] == "yes"
	appID := strings.Join(parts[2:], "_")
	appID = "apr_" + appID

	if b.approvals != nil {
		app, err := b.approvals.Resolve(appID, approved, "telegram")
		statusText := "❌ Denied"
		if approved {
			statusText = "✅ Approved"
		}
		if err != nil {
			statusText = fmt.Sprintf("⚠️ %v", err)
		}

		_, _ = bot.AnswerCallbackQuery(ctx, &tgbot.AnswerCallbackQueryParams{
			CallbackQueryID: update.CallbackQuery.ID,
			Text:            statusText,
		})

		if app != nil && update.CallbackQuery.Message.Message != nil {
			origMsg := update.CallbackQuery.Message.Message
			newText := fmt.Sprintf("%s\n\n*Resolution:* %s (via Telegram)", origMsg.Text, statusText)
			_, _ = bot.EditMessageText(ctx, &tgbot.EditMessageTextParams{
				ChatID:    origMsg.Chat.ID,
				MessageID: origMsg.ID,
				Text:      newText,
				ParseMode: models.ParseModeMarkdown,
			})
		}
	}
}

// NotifyApproval sends an interactive inline keyboard message for an approval request.
func (b *Bot) NotifyApproval(ctx context.Context, chatID int64, app *approval.PendingApproval) {
	b.mu.RLock()
	bot := b.bot
	running := b.running
	b.mu.RUnlock()

	if !running || bot == nil || chatID == 0 {
		return
	}

	rawID := strings.TrimPrefix(app.ID, "apr_")
	kb := &models.InlineKeyboardMarkup{
		InlineKeyboard: [][]models.InlineKeyboardButton{
			{
				{Text: "✅ Approve", CallbackData: "apr_yes_" + rawID},
				{Text: "❌ Deny", CallbackData: "apr_no_" + rawID},
			},
		},
	}

	msg := fmt.Sprintf("⚠️ *Action Approval Required*\n\n*Session:* `%s`\n*Tool:* `%s`\n*Command:* `%s`\n\n_Auto-denies in %d seconds if unanswered._",
		app.SessionID, app.ToolName, app.Command, app.AutoDenyTimeoutMs/1000)

	_, _ = bot.SendMessage(ctx, &tgbot.SendMessageParams{
		ChatID:      chatID,
		Text:        msg,
		ParseMode:   models.ParseModeMarkdown,
		ReplyMarkup: kb,
	})
}

// NotifyChatMessage broadcasts a completed assistant chat message to the configured chat.
func (b *Bot) NotifyChatMessage(ctx context.Context, chatID int64, msg chat.Message) {
	b.mu.RLock()
	bot := b.bot
	running := b.running
	b.mu.RUnlock()

	if !running || bot == nil || chatID == 0 || msg.Streaming || msg.Role != protocol.RoleAssistant {
		return
	}

	text := msg.Text
	if len(text) > 4000 {
		text = text[:4000] + "..."
	}

	_, _ = bot.SendMessage(ctx, &tgbot.SendMessageParams{
		ChatID: chatID,
		Text:   fmt.Sprintf("💬 *[%s]*\n%s", msg.SessionID, text),
	})
}

func (b *Bot) Status() Status {
	b.mu.RLock()
	defer b.mu.RUnlock()
	return Status{
		Enabled:   b.cfg.Token != "",
		Running:   b.running,
		LastError: b.lastError,
	}
}
