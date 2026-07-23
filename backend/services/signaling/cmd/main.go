// Command signaling brokers WebRTC connection setup between paired peers over
// authenticated, secure WebSockets. It relays SDP offers/answers and ICE
// candidates and reports peer presence; it never sees decrypted media. Access
// is gated by short-lived signaling tickets issued by the session service, so
// the service is stateless and horizontally scalable.
package main

import (
	"os"

	"github.com/desksync/backend/pkg/config"
	"github.com/desksync/backend/pkg/logger"
	"github.com/desksync/backend/pkg/service"
	"github.com/desksync/backend/pkg/signalticket"
	"github.com/desksync/backend/services/signaling/internal/hub"
	"github.com/desksync/backend/services/signaling/internal/ws"
	"github.com/gofiber/fiber/v2"
)

var version = "0.3.0-phase5"

func main() {
	base := config.LoadBase("signaling", "SIGNALING_HTTP_ADDR", ":8085")
	log := logger.New(logger.Options{ServiceName: base.ServiceName, Level: base.LogLevel, Format: base.LogFormat})

	sigCfg := config.LoadSignaling()
	verifier, err := signalticket.NewVerifier(sigCfg.TicketSecret)
	if err != nil {
		log.Error("invalid signaling ticket configuration", "error", err.Error())
		os.Exit(1)
	}

	relayHub := hub.New(log)
	wsHandler := ws.New(relayHub, verifier, log)

	service.Run(service.Spec{
		Name:        "signaling",
		HTTPAddrEnv: "SIGNALING_HTTP_ADDR",
		DefaultAddr: ":8085",
		Version:     version,
	}, func(app *fiber.App, _ service.Deps) {
		wsHandler.Register(app.Group("/api/v1"))
	})
}
