package protocol

import "testing"

func TestParseValidEnvelope(t *testing.T) {
	raw := []byte(`{"v":1,"nonce":5,"ts_ms":1,"session_id":"s1","payload":{"kind":"offer","sdp":"x"}}`)
	e, err := Parse(raw)
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if e.SessionID != "s1" || e.Nonce != 5 {
		t.Fatalf("unexpected envelope %+v", e)
	}
	if e.Kind() != KindOffer {
		t.Fatalf("kind = %q, want offer", e.Kind())
	}
}

func TestParseRejectsInvalid(t *testing.T) {
	cases := [][]byte{
		[]byte(`not json`),
		[]byte(`{"v":2,"session_id":"s","payload":{"kind":"offer"}}`), // wrong version
		[]byte(`{"v":1,"session_id":"","payload":{"kind":"offer"}}`),  // no session
		[]byte(`{"v":1,"session_id":"s"}`),                            // no payload
	}
	for i, c := range cases {
		if _, err := Parse(c); err == nil {
			t.Fatalf("case %d: expected error", i)
		}
	}
}

func TestControlEnvelopeRoundTrips(t *testing.T) {
	b := Control("s1", KindPeerJoined, "agent")
	e, err := Parse(b)
	if err != nil {
		t.Fatalf("Parse control: %v", err)
	}
	if e.Kind() != KindPeerJoined {
		t.Fatalf("kind = %q, want peer_joined", e.Kind())
	}
}

func TestNonceGuard(t *testing.T) {
	var g NonceGuard
	if !g.Accept(1) {
		t.Fatal("first nonce should be accepted")
	}
	if !g.Accept(2) {
		t.Fatal("increasing nonce should be accepted")
	}
	if g.Accept(2) {
		t.Fatal("replayed nonce must be rejected")
	}
	if g.Accept(1) {
		t.Fatal("reordered nonce must be rejected")
	}
	if !g.Accept(3) {
		t.Fatal("increasing nonce should be accepted")
	}
}
