package errors

import (
	stderrors "errors"
	"net/http"
	"testing"
)

func TestHTTPStatusMapping(t *testing.T) {
	cases := map[Code]int{
		CodeInvalidInput:  http.StatusBadRequest,
		CodeUnauthorized:  http.StatusUnauthorized,
		CodeForbidden:     http.StatusForbidden,
		CodeNotFound:      http.StatusNotFound,
		CodeConflict:      http.StatusConflict,
		CodeRateLimited:   http.StatusTooManyRequests,
		CodeUnavailable:   http.StatusServiceUnavailable,
		CodePreconditionF: http.StatusPreconditionFailed,
		CodeInternal:      http.StatusInternalServerError,
	}
	for code, want := range cases {
		if got := New(code, "x").HTTPStatus(); got != want {
			t.Errorf("code %s: HTTPStatus = %d, want %d", code, got, want)
		}
	}
}

func TestWrapUnwrapAndAs(t *testing.T) {
	cause := stderrors.New("boom")
	err := Wrap(CodeInternal, "failed", cause)

	if !stderrors.Is(err, cause) {
		t.Fatal("errors.Is could not find wrapped cause")
	}
	if de, ok := As(err); !ok || de.Code != CodeInternal {
		t.Fatalf("As = (%v, %v)", de, ok)
	}
	if got := err.Error(); got == "" {
		t.Fatal("Error() returned empty string")
	}
}
