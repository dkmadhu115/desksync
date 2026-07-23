package httpx

import (
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/logger"
	"github.com/desksync/backend/pkg/observability"
)

func newTestApp(t *testing.T) *fiberAppUnderTest {
	t.Helper()
	base := config.Base{ServiceName: "test-svc", Environment: config.EnvDevelopment}
	app := New(Options{
		Base:    base,
		Logger:  logger.New(logger.Options{ServiceName: "test-svc", Level: "error", Format: "json"}),
		Metrics: observability.NewMetrics("test-svc"),
		Version: "test",
	})
	return &fiberAppUnderTest{app: app, t: t}
}

// fiberAppUnderTest is a thin wrapper to keep the test readable.
type fiberAppUnderTest struct {
	app interface {
		Test(*http.Request, ...int) (*http.Response, error)
	}
	t *testing.T
}

func (f *fiberAppUnderTest) get(path string) (*http.Response, string) {
	f.t.Helper()
	req := httptest.NewRequest(http.MethodGet, path, nil)
	resp, err := f.app.Test(req, -1)
	if err != nil {
		f.t.Fatalf("request to %s failed: %v", path, err)
	}
	body, _ := io.ReadAll(resp.Body)
	_ = resp.Body.Close()
	return resp, string(body)
}

func TestHealthEndpoint(t *testing.T) {
	app := newTestApp(t)
	resp, body := app.get("/health")
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("/health status = %d, want 200", resp.StatusCode)
	}
	if !strings.Contains(body, `"status":"ok"`) {
		t.Fatalf("/health body missing status: %s", body)
	}
	if !strings.Contains(body, `"service":"test-svc"`) {
		t.Fatalf("/health body missing service: %s", body)
	}
}

func TestReadyEndpoint(t *testing.T) {
	app := newTestApp(t)
	resp, _ := app.get("/ready")
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("/ready status = %d, want 200", resp.StatusCode)
	}
}

func TestMetricsEndpoint(t *testing.T) {
	app := newTestApp(t)
	// Hit /health first so the counter is non-zero.
	app.get("/health")
	resp, body := app.get("/metrics")
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("/metrics status = %d, want 200", resp.StatusCode)
	}
	if !strings.Contains(body, "http_requests_total") {
		t.Fatalf("/metrics missing http_requests_total: %s", body)
	}
}
