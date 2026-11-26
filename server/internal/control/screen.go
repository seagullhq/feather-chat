package control

import (
	"strings"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/protocol"
)

// A viewer may ask one presenter for a keyframe at most twice a second.
// Fixed for v1; matches the client's KEY_REQ_GAP.
const keyReqGap = 500 * time.Millisecond

// keyReqLimiter throttles SCREEN_KEYREQ per viewer and presenter. It is owned
// by one control connection, so it needs no lock.
type keyReqLimiter struct {
	last map[uint32]time.Time
}

func (l *keyReqLimiter) allow(ssrc uint32, now time.Time) bool {
	if l.last == nil {
		l.last = make(map[uint32]time.Time)
	}
	if now.Sub(l.last[ssrc]) < keyReqGap {
		return false
	}
	l.last[ssrc] = now
	return true
}

// handleScreenStart answers SCREEN_START: the session begins sharing its screen.
func (s *Server) handleScreenStart(c *client, _ protocol.Control) {
	me, ok := s.rooms.StartShare(c.sess)
	if !ok {
		c.sendError("cannot share")
		return
	}
	c.send(protocol.Control{Type: "ACK", SSRC: c.sess.SSRC})
	if strings.HasPrefix(c.sess.Room, "dm:") {
		s.notifyDmCallState(c, me)
		return
	}
	s.notifyServer(c.sess.ServerID, c.sess, protocol.Control{Type: "MEMBER_STATE", ChannelID: c.sess.ChannelID, Member: &me})
}

// handleScreenStop answers SCREEN_STOP. Also runs implicitly on LEAVE (dropLocked).
func (s *Server) handleScreenStop(c *client, _ protocol.Control) {
	if c.sess.Room == "" {
		return
	}
	me := s.rooms.StopShare(c.sess)
	if strings.HasPrefix(c.sess.Room, "dm:") {
		s.notifyDmCallState(c, me)
		return
	}
	s.notifyServer(c.sess.ServerID, c.sess, protocol.Control{Type: "MEMBER_STATE", ChannelID: c.sess.ChannelID, Member: &me})
}

// handleScreenWatch answers SCREEN_WATCH{ssrc}: subscribe to a room-mate's share
// and ask the presenter for a keyframe so the viewer can start decoding.
func (s *Server) handleScreenWatch(c *client, msg protocol.Control) {
	t := s.rooms.Watch(c.sess, msg.SSRC)
	if t == nil {
		c.sendError("not sharing")
		return
	}
	c.send(protocol.Control{Type: "ACK", SSRC: msg.SSRC})
	_ = t.Send(protocol.Control{Type: "SCREEN_KEYREQ"})
}

// handleScreenUnwatch answers SCREEN_UNWATCH{ssrc}.
func (s *Server) handleScreenUnwatch(c *client, msg protocol.Control) {
	s.rooms.Unwatch(c.sess, msg.SSRC)
}

// handleScreenKeyReq answers SCREEN_KEYREQ{ssrc}: a viewer lost frames and asks
// the presenter for a keyframe. Requests are throttled per viewer and presenter
// so a broken decoder cannot flood the presenter with keyframes.
func (s *Server) handleScreenKeyReq(c *client, msg protocol.Control) {
	if !c.keyReq.allow(msg.SSRC, time.Now()) {
		return
	}
	if t := s.rooms.Watch(c.sess, msg.SSRC); t != nil {
		_ = t.Send(protocol.Control{Type: "SCREEN_KEYREQ"})
	}
}
