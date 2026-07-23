package domain

import "errors"

// Sentinel errors surfaced by the domain/repository layer. The application
// layer maps these to pkg/errors codes for the HTTP layer.
var (
	// ErrUserNotFound is returned when a user does not exist.
	ErrUserNotFound = errors.New("user not found")
	// ErrEmailTaken is returned when registering an already-used email.
	ErrEmailTaken = errors.New("email already registered")
	// ErrRefreshNotFound is returned when a refresh token JTI is unknown.
	ErrRefreshNotFound = errors.New("refresh token not found")
)
