package postgres

import (
	"testing"

	"github.com/desksync/backend/pkg/config"
)

func TestDSNPrefersURL(t *testing.T) {
	c := config.PostgresConfig{URL: "postgres://u:p@h:5432/db?sslmode=require"}
	if got := DSN(c); got != c.URL {
		t.Fatalf("DSN = %q, want %q", got, c.URL)
	}
}

func TestDSNBuildsFromParts(t *testing.T) {
	c := config.PostgresConfig{
		Host: "localhost", Port: 5432, User: "desksync",
		Password: "secret", Database: "desksync", SSLMode: "disable",
	}
	want := "postgres://desksync:secret@localhost:5432/desksync?sslmode=disable"
	if got := DSN(c); got != want {
		t.Fatalf("DSN = %q, want %q", got, want)
	}
}
