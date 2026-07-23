// Package logger provides a structured, JSON-first logger built on the standard
// library's log/slog. Every service uses this so logs are uniformly shaped
// (service name, level, timestamp) and machine-parseable for Loki.
package logger

import (
	"context"
	"log/slog"
	"os"
	"strings"
)

// contextKey is unexported to avoid collisions in context.Value.
type contextKey struct{ name string }

var loggerKey = contextKey{name: "desksync-logger"}

// Options configures a new logger.
type Options struct {
	ServiceName string
	Level       string // debug | info | warn | error
	Format      string // json | console
}

// New builds a *slog.Logger with the service name pre-attached as a field.
func New(opts Options) *slog.Logger {
	handlerOpts := &slog.HandlerOptions{
		Level:     parseLevel(opts.Level),
		AddSource: false,
	}

	var handler slog.Handler
	if strings.EqualFold(opts.Format, "console") {
		handler = slog.NewTextHandler(os.Stdout, handlerOpts)
	} else {
		handler = slog.NewJSONHandler(os.Stdout, handlerOpts)
	}

	return slog.New(handler).With(slog.String("service", opts.ServiceName))
}

// parseLevel maps a string level to slog.Level, defaulting to Info.
func parseLevel(level string) slog.Level {
	switch strings.ToLower(strings.TrimSpace(level)) {
	case "debug":
		return slog.LevelDebug
	case "warn", "warning":
		return slog.LevelWarn
	case "error":
		return slog.LevelError
	default:
		return slog.LevelInfo
	}
}

// WithContext stores a logger in the context for downstream retrieval.
func WithContext(ctx context.Context, l *slog.Logger) context.Context {
	return context.WithValue(ctx, loggerKey, l)
}

// FromContext returns the logger stored in ctx, or slog.Default when absent.
func FromContext(ctx context.Context) *slog.Logger {
	if l, ok := ctx.Value(loggerKey).(*slog.Logger); ok && l != nil {
		return l
	}
	return slog.Default()
}
