package control

import (
	"io"
	"net"
	"net/netip"
	"slices"
	"sync"
	"time"

	"golang.org/x/time/rate"
)

// Control channel limits (transport-security.md §1.3).
const (
	handshakeTimeout = 10 * time.Second
	preAuthTimeout   = 60 * time.Second
	preAuthMaxFrame  = 4096
	maxConns         = 1024
	maxConnsPerIP    = 16
	maxUnauthConns   = 256
	authBurst        = 10
	authEvery        = 6 * time.Second
	failedBurst      = 5
	failedEvery      = 30 * time.Second
	// Failures of one account from all sources together. Much larger than
	// failedBurst, so one source cannot lock the owner out.
	accountBurst = 50
	accountEvery = 6 * time.Second
	// Sources remembered for having logged in.
	maxKnownIPs = 4096
)

const tooManyAttempts = "too many attempts, try again later"

// ipKey names the source of a connection: the IPv4 address, or the /64 of an
// IPv6 one.
func ipKey(addr net.Addr) string {
	ap, err := netip.ParseAddrPort(addr.String())
	if err != nil {
		return addr.String()
	}
	ip := ap.Addr().Unmap()
	if ip.Is6() {
		p, _ := ip.Prefix(64) // cannot fail for an IPv6 address
		return p.String()
	}
	return ip.String()
}

// connLimiter counts open connections, in total, per source and not yet
// logged in.
type connLimiter struct {
	mu    sync.Mutex
	total int
	perIP map[string]int
	// unauth lists the connections that have not logged in, oldest first.
	unauth []*connSlot
	// known holds when each source last logged in successfully. Its waiting
	// connections are the last to be evicted.
	known map[string]time.Time
}

// connSlot is the place one connection holds in the limiter.
type connSlot struct {
	ip       string
	conn     io.Closer
	released bool // guarded by connLimiter.mu
}

// acquire takes a slot for conn; every success needs one release. If the
// unauthenticated or the total limit is reached, an unauthenticated connection
// is closed to make room, so waiting connections cannot shut newcomers out.
func (l *connLimiter) acquire(ip string, conn io.Closer) (*connSlot, bool) {
	l.mu.Lock()
	full := len(l.unauth) >= maxUnauthConns || l.total >= maxConns
	if l.perIP[ip] >= maxConnsPerIP || (full && len(l.unauth) == 0) {
		l.mu.Unlock()
		return nil, false
	}
	var victim *connSlot
	if full {
		victim = l.victimLocked()
		l.releaseLocked(victim)
	}
	if l.perIP == nil {
		l.perIP = map[string]int{}
	}
	slot := &connSlot{ip: ip, conn: conn}
	l.total++
	l.perIP[ip]++
	l.unauth = append(l.unauth, slot)
	l.mu.Unlock()
	if victim != nil {
		_ = victim.conn.Close()
	}
	return slot, true
}

// victimLocked picks the connection to evict. Sources that have logged in
// before are spared while others wait. Then the network (IPv4 /24, IPv6 /48)
// holding the most loses, within it the source holding the most, and within that
// the oldest connection. unauth must not be empty.
func (l *connLimiter) victimLocked() *connSlot {
	inNet := make(map[string]int, len(l.unauth))
	held := make(map[string]int, len(l.unauth))
	for _, s := range l.unauth {
		inNet[netKey(s.ip)]++
		held[s.ip]++
	}
	rank := func(s *connSlot) [3]int {
		_, known := l.known[s.ip]
		stranger := 1
		if known {
			stranger = 0
		}
		return [3]int{stranger, inNet[netKey(s.ip)], held[s.ip]}
	}
	return slices.MaxFunc(l.unauth, func(a, b *connSlot) int {
		x, y := rank(a), rank(b)
		return slices.Compare(x[:], y[:]) // MaxFunc keeps the first, oldest, of equals
	})
}

// netKey widens an ipKey to the network that is likely under one control.
func netKey(ip string) string {
	if a, err := netip.ParseAddr(ip); err == nil {
		p, _ := a.Prefix(24) // cannot fail for an IPv4 address
		return p.String()
	}
	if p, err := netip.ParsePrefix(ip); err == nil {
		wide, _ := p.Addr().Prefix(48) // cannot fail for an IPv6 address
		return wide.String()
	}
	return ip
}

// promote marks a connection as logged in: it can no longer be evicted, and its
// source is remembered.
func (l *connLimiter) promote(slot *connSlot) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.dropUnauthLocked(slot)
	if l.known == nil {
		l.known = map[string]time.Time{}
	}
	l.known[slot.ip] = time.Now()
	if len(l.known) > maxKnownIPs {
		var oldest string
		var at time.Time
		for ip, seen := range l.known {
			if oldest == "" || seen.Before(at) {
				oldest, at = ip, seen
			}
		}
		delete(l.known, oldest)
	}
}

func (l *connLimiter) dropUnauthLocked(slot *connSlot) {
	if i := slices.Index(l.unauth, slot); i >= 0 {
		l.unauth = slices.Delete(l.unauth, i, i+1)
	}
}

// release frees a slot. An evicted slot is already free.
func (l *connLimiter) release(slot *connSlot) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.releaseLocked(slot)
}

func (l *connLimiter) releaseLocked(slot *connSlot) {
	if slot.released {
		return
	}
	slot.released = true
	l.dropUnauthLocked(slot)
	l.total--
	if l.perIP[slot.ip]--; l.perIP[slot.ip] == 0 {
		delete(l.perIP, slot.ip)
	}
}

// limiterSweep is how often idle buckets are dropped.
const limiterSweep = time.Minute

// keyedLimiter holds one token bucket per key. A bucket that refilled
// completely equals a new one, so it is dropped and the map stays bounded.
type keyedLimiter struct {
	every time.Duration
	burst int

	mu      sync.Mutex
	buckets map[string]*bucket
	swept   time.Time
}

type bucket struct {
	tokens *rate.Limiter
	// pending counts reservations whose outcome is not known yet.
	pending int
}

func newKeyedLimiter(burst int, every time.Duration) *keyedLimiter {
	return &keyedLimiter{every: every, burst: burst, buckets: map[string]*bucket{}}
}

func (l *keyedLimiter) bucketLocked(key string, now time.Time) *bucket {
	if now.Sub(l.swept) >= limiterSweep {
		for k, b := range l.buckets {
			if b.pending == 0 && b.tokens.TokensAt(now) >= float64(l.burst) {
				delete(l.buckets, k)
			}
		}
		l.swept = now
	}
	b := l.buckets[key]
	if b == nil {
		b = &bucket{tokens: rate.NewLimiter(rate.Every(l.every), l.burst)}
		l.buckets[key] = b
	}
	return b
}

// allow spends one token of key, if there is one.
func (l *keyedLimiter) allow(key string, now time.Time) bool {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.bucketLocked(key, now).tokens.AllowN(now, 1)
}

// reserve serves limits that count failures only. It holds a token while the
// attempt runs, so parallel attempts cannot overrun the bucket; done spends it
// if the attempt failed and must be called exactly once.
func (l *keyedLimiter) reserve(key string, now time.Time) (done func(failed bool), ok bool) {
	l.mu.Lock()
	defer l.mu.Unlock()
	b := l.bucketLocked(key, now)
	if b.tokens.TokensAt(now)-float64(b.pending) < 1 {
		return nil, false
	}
	b.pending++
	return func(failed bool) {
		l.mu.Lock()
		defer l.mu.Unlock()
		b.pending--
		if failed {
			b.tokens.Allow()
		}
	}, true
}
