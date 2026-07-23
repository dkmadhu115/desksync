package transport

import "github.com/desksync/backend/services/auth/internal/service"

// registerRequest is the body for POST /auth/register.
type registerRequest struct {
	Email       string `json:"email"`
	Password    string `json:"password"`
	DisplayName string `json:"display_name"`
}

// loginRequest is the body for POST /auth/login.
type loginRequest struct {
	Email    string `json:"email"`
	Password string `json:"password"`
}

// refreshRequest is the body for POST /auth/refresh and /auth/logout.
type refreshRequest struct {
	RefreshToken string `json:"refresh_token"`
}

// tokenResponse is the standard authentication result.
type tokenResponse struct {
	AccessToken  string  `json:"access_token"`
	RefreshToken string  `json:"refresh_token"`
	TokenType    string  `json:"token_type"`
	ExpiresIn    int     `json:"expires_in"`
	User         userDTO `json:"user"`
}

type userDTO struct {
	ID          string `json:"id"`
	Email       string `json:"email"`
	DisplayName string `json:"display_name"`
}

func toTokenResponse(t service.Tokens) tokenResponse {
	return tokenResponse{
		AccessToken:  t.AccessToken,
		RefreshToken: t.RefreshToken,
		TokenType:    "Bearer",
		ExpiresIn:    t.ExpiresIn,
		User: userDTO{
			ID:          t.User.ID,
			Email:       t.User.Email,
			DisplayName: t.User.DisplayName,
		},
	}
}
