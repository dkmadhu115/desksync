// Package errors defines the canonical error type used across DeskSync
// services. Domain and transport layers raise *Error values so the HTTP layer
// can translate them into consistent JSON responses and status codes without
// leaking internal details to clients.
package errors

import (
	"errors"
	"fmt"
	"net/http"
)

// Code is a stable, machine-readable error identifier returned to clients.
type Code string

const (
	CodeInvalidInput  Code = "invalid_input"
	CodeUnauthorized  Code = "unauthorized"
	CodeForbidden     Code = "forbidden"
	CodeNotFound      Code = "not_found"
	CodeConflict      Code = "conflict"
	CodeRateLimited   Code = "rate_limited"
	CodeInternal      Code = "internal_error"
	CodeUnavailable   Code = "service_unavailable"
	CodePreconditionF Code = "precondition_failed"
)

// Error is the canonical application error. It carries a client-safe code and
// message plus an optional wrapped cause for logging.
type Error struct {
	Code    Code
	Message string
	cause   error
}

// New creates an Error with the given code and message.
func New(code Code, message string) *Error {
	return &Error{Code: code, Message: message}
}

// Wrap creates an Error that wraps an underlying cause.
func Wrap(code Code, message string, cause error) *Error {
	return &Error{Code: code, Message: message, cause: cause}
}

func (e *Error) Error() string {
	if e.cause != nil {
		return fmt.Sprintf("%s: %s: %v", e.Code, e.Message, e.cause)
	}
	return fmt.Sprintf("%s: %s", e.Code, e.Message)
}

// Unwrap exposes the wrapped cause for errors.Is / errors.As.
func (e *Error) Unwrap() error { return e.cause }

// HTTPStatus maps the error code to an HTTP status code.
func (e *Error) HTTPStatus() int {
	switch e.Code {
	case CodeInvalidInput:
		return http.StatusBadRequest
	case CodeUnauthorized:
		return http.StatusUnauthorized
	case CodeForbidden:
		return http.StatusForbidden
	case CodeNotFound:
		return http.StatusNotFound
	case CodeConflict:
		return http.StatusConflict
	case CodePreconditionF:
		return http.StatusPreconditionFailed
	case CodeRateLimited:
		return http.StatusTooManyRequests
	case CodeUnavailable:
		return http.StatusServiceUnavailable
	default:
		return http.StatusInternalServerError
	}
}

// As extracts an *Error from err, returning it and true when present.
func As(err error) (*Error, bool) {
	var e *Error
	if errors.As(err, &e) {
		return e, true
	}
	return nil, false
}
