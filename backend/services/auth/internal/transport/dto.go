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

// desktopExchangeRequest is the body for POST /auth/oauth/desktop/exchange: the
// one-time code the browser handed to the desktop's loopback listener, plus the
// PKCE verifier proving this is the process that started the sign-in.
type desktopExchangeRequest struct {
	Code         string `json:"code"`
	CodeVerifier string `json:"code_verifier"`
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
