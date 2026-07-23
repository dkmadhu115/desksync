package httpx

import (
	"net/http"
	"strconv"

	"github.com/gofiber/fiber/v2"
	"github.com/valyala/fasthttp/fasthttpadaptor"
)

// adaptHTTP converts a standard net/http handler into a Fiber handler. Used to
// mount the Prometheus HTTP handler on a Fiber route.
func adaptHTTP(h http.HandlerFunc) fiber.Handler {
	converted := fasthttpadaptor.NewFastHTTPHandler(h)
	return func(c *fiber.Ctx) error {
		converted(c.Context())
		return nil
	}
}

// statusText returns the numeric status code as a string for metric labels.
// Using the raw code keeps cardinality bounded and avoids leaking messages.
func statusText(code int) string {
	return strconv.Itoa(code)
}
