package control

import "github.com/seagullhq/feather-chat/server/internal/protocol"

// handleScreenStart answers SCREEN_START: the session begins sharing its screen.
func (s *Server) handleScreenStart(c *client, _ protocol.Control) {
	me, ok := s.rooms.StartShare(c.sess)
	if !ok {
		c.sendError("cannot share")
		return
	}
	c.send(protocol.Control{Type: "ACK", SSRC: c.sess.SSRC})
	s.notifyServer(c.sess.ServerID, c.sess, protocol.Control{Type: "MEMBER_STATE", ChannelID: c.sess.ChannelID, Member: &me})
}

// handleScreenStop answers SCREEN_STOP. Also runs implicitly on LEAVE (dropLocked).
func (s *Server) handleScreenStop(c *client, _ protocol.Control) {
	if c.sess.Room == "" {
		return
	}
	me := s.rooms.StopShare(c.sess)
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
// the presenter for a keyframe. ponytail: no rate limit, add one if viewers spam it.
func (s *Server) handleScreenKeyReq(c *client, msg protocol.Control) {
	if t := s.rooms.Watch(c.sess, msg.SSRC); t != nil {
		_ = t.Send(protocol.Control{Type: "SCREEN_KEYREQ"})
	}
}
