// Package ice builds the ICE server list returned to clients so they can
// establish a WebRTC connection: public STUN servers plus, when configured,
// TURN servers with short-lived, per-session credentials.
//
// TURN credentials use the coturn "use-auth-secret" (TURN REST API) scheme:
// the username is "<expiryUnix>:<sessionID>" and the credential is
// base64(HMAC-SHA1(secret, username)). The static secret is shared with the
// TURN server out of band and NEVER sent to clients; only derived, expiring
// credentials are. This means a leaked credential is useless after it expires
// and cannot be traced back to the secret.
package ice

import (
	"crypto/hmac"
	"crypto/sha1"
	"encoding/base64"
	"fmt"
	"time"

	"github.com/desksync/backend/pkg/config"
)

// Server is a single ICE server entry (STUN or TURN).
type Server struct {
	URLs       []string `json:"urls"`
	Username   string   `json:"username,omitempty"`
	Credential string   `json:"credential,omitempty"`
}

// Builder constructs ICE server lists from configuration.
type Builder struct {
	cfg config.ICEConfig
	now func() time.Time
}

// NewBuilder builds a Builder.
func NewBuilder(cfg config.ICEConfig) *Builder {
	return &Builder{cfg: cfg, now: time.Now}
}

// Build returns the ICE servers for a session. STUN servers are always
// included when configured; TURN servers are included only when both TURN URLs
// and a static auth secret are present.
func (b *Builder) Build(sessionID string) []Server {
	servers := make([]Server, 0, 2)

	if len(b.cfg.STUNURLs) > 0 {
		servers = append(servers, Server{URLs: append([]string(nil), b.cfg.STUNURLs...)})
	}

	if len(b.cfg.TURNURLs) > 0 && b.cfg.TURNSecret != "" {
		username, credential := b.turnCredentials(sessionID)
		servers = append(servers, Server{
			URLs:       append([]string(nil), b.cfg.TURNURLs...),
			Username:   username,
			Credential: credential,
		})
	}

	return servers
}

// turnCredentials derives a time-limited TURN username/credential pair.
func (b *Builder) turnCredentials(sessionID string) (string, string) {
	ttl := b.cfg.CredentialTTL
	if ttl <= 0 {
		ttl = time.Hour
	}
	expiry := b.now().Add(ttl).Unix()
	username := fmt.Sprintf("%d:%s", expiry, sessionID)

	mac := hmac.New(sha1.New, []byte(b.cfg.TURNSecret))
	mac.Write([]byte(username))
	credential := base64.StdEncoding.EncodeToString(mac.Sum(nil))
	return username, credential
}
