package control

import (
	"testing"
	"time"
)

func TestKeyReqLimiter(t *testing.T) {
	base := time.Now()
	var l keyReqLimiter

	if !l.allow(7, base) {
		t.Fatal("first request must be allowed")
	}
	if l.allow(7, base.Add(keyReqGap-time.Nanosecond)) {
		t.Fatal("request inside the gap must be dropped")
	}
	if !l.allow(7, base.Add(keyReqGap)) {
		t.Fatal("request at the end of the gap must be allowed")
	}

	// A different presenter has its own budget.
	if !l.allow(8, base.Add(keyReqGap/2)) {
		t.Fatal("limit must be per presenter")
	}
	// The map is lazily created, so the zero value is usable.
	var fresh keyReqLimiter
	if !fresh.allow(9, base) {
		t.Fatal("zero-value limiter must allow the first request")
	}
}
