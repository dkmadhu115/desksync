// Package observability centralizes Prometheus metric registration for every
// service. Each service creates one Metrics instance at startup and shares it
// with the HTTP layer so request counts and latencies are recorded uniformly.
package observability

import (
	"github.com/prometheus/client_golang/prometheus"
)

// Metrics bundles the standard RED (Rate, Errors, Duration) metrics that every
// service exposes, plus a dedicated registry to avoid global-state collisions
// in tests.
type Metrics struct {
	Registry        *prometheus.Registry
	RequestsTotal   *prometheus.CounterVec
	RequestDuration *prometheus.HistogramVec
	InFlight        prometheus.Gauge
}

// NewMetrics builds and registers the standard metric set for a service.
func NewMetrics(service string) *Metrics {
	reg := prometheus.NewRegistry()
	constLabels := prometheus.Labels{"service": service}

	m := &Metrics{
		Registry: reg,
		RequestsTotal: prometheus.NewCounterVec(prometheus.CounterOpts{
			Name:        "http_requests_total",
			Help:        "Total number of HTTP requests processed, labeled by method, route and status.",
			ConstLabels: constLabels,
		}, []string{"method", "route", "status"}),
		RequestDuration: prometheus.NewHistogramVec(prometheus.HistogramOpts{
			Name:        "http_request_duration_seconds",
			Help:        "HTTP request latency distribution in seconds.",
			ConstLabels: constLabels,
			Buckets:     prometheus.DefBuckets,
		}, []string{"method", "route"}),
		InFlight: prometheus.NewGauge(prometheus.GaugeOpts{
			Name:        "http_requests_in_flight",
			Help:        "Number of HTTP requests currently being served.",
			ConstLabels: constLabels,
		}),
	}

	// Register Go runtime and process collectors alongside our custom metrics.
	reg.MustRegister(
		m.RequestsTotal,
		m.RequestDuration,
		m.InFlight,
		prometheus.NewGoCollector(),
		prometheus.NewProcessCollector(prometheus.ProcessCollectorOpts{}),
	)

	return m
}
