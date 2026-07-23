package config

import (
	"testing"
	"time"
)

func TestGetString(t *testing.T) {
	t.Setenv("DS_TEST_STR", "hello")
	if got := GetString("DS_TEST_STR", "def"); got != "hello" {
		t.Fatalf("GetString = %q, want %q", got, "hello")
	}
	if got := GetString("DS_TEST_MISSING", "def"); got != "def" {
		t.Fatalf("GetString default = %q, want %q", got, "def")
	}
}

func TestGetIntBoolDuration(t *testing.T) {
	t.Setenv("DS_TEST_INT", "42")
	t.Setenv("DS_TEST_BOOL", "true")
	t.Setenv("DS_TEST_DUR", "5s")

	if got := GetInt("DS_TEST_INT", 1); got != 42 {
		t.Fatalf("GetInt = %d, want 42", got)
	}
	if got := GetInt("DS_TEST_BAD_INT", 7); got != 7 {
		t.Fatalf("GetInt default = %d, want 7", got)
	}
	if got := GetBool("DS_TEST_BOOL", false); !got {
		t.Fatalf("GetBool = %v, want true", got)
	}
	if got := GetDuration("DS_TEST_DUR", time.Second); got != 5*time.Second {
		t.Fatalf("GetDuration = %v, want 5s", got)
	}
}

func TestLoadBaseAndProduction(t *testing.T) {
	t.Setenv("ENVIRONMENT", "production")
	t.Setenv("GATEWAY_HTTP_ADDR", ":9999")
	b := LoadBase("gateway", "GATEWAY_HTTP_ADDR", ":8080")

	if b.ServiceName != "gateway" {
		t.Fatalf("ServiceName = %q", b.ServiceName)
	}
	if b.HTTPAddr != ":9999" {
		t.Fatalf("HTTPAddr = %q, want :9999", b.HTTPAddr)
	}
	if !b.IsProduction() {
		t.Fatal("IsProduction = false, want true")
	}
}

func TestMustGet(t *testing.T) {
	t.Setenv("DS_TEST_SECRET", "s3cret")
	if v, err := MustGet("DS_TEST_SECRET"); err != nil || v != "s3cret" {
		t.Fatalf("MustGet = (%q, %v)", v, err)
	}
	if _, err := MustGet("DS_TEST_ABSENT"); err == nil {
		t.Fatal("MustGet(absent) error = nil, want error")
	}
}
