// Package control implements the TCP control plane: it accepts client
// connections, decodes §7 control messages, and routes each type to its
// handler.
package control

import (
	"errors"
	"log"
	"net"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/friends"
	"github.com/seagullhq/feather-chat/server/internal/messages"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/room"
	"github.com/seagullhq/feather-chat/server/internal/servers"
)

// Server is the control plane: the room state, the persistence stores, and
// one accept loop feeding per-connection goroutines.
type Server struct {
	rooms    *room.Manager
	calls    *callManager
	users    *accounts.Store
	servers  *servers.ServerStore
	friends  *friends.ServerStore
	messages *messages.ServerStore
}

// New wires a control server around the stores and the room state.
func New(users *accounts.Store, serverStore *servers.ServerStore, friendStore *friends.ServerStore, msgStore *messages.ServerStore, rooms *room.Manager) *Server {
	return &Server{rooms: rooms, calls: newCallManager(), users: users, servers: serverStore, friends: friendStore, messages: msgStore}
}

// Serve accepts and dispatches control connections until the listener stops.
func (s *Server) Serve(l net.Listener) {
	for {
		conn, err := l.Accept()
		if err != nil {
			if !errors.Is(err, net.ErrClosed) {
				log.Printf("[CONTROL] accept failed: %v", err)
			}
			return
		}
		go s.handleConn(conn)
	}
}

// handlers routes every control message type to its handler. Unknown types
// fall through silently, per the §7 interop rule.
var handlers = map[string]func(*Server, *client, protocol.Control){
	"REGISTER":                   (*Server).handleRegister,
	"LOGIN":                      (*Server).handleLogin,
	"HELLO":                      (*Server).handleHello,
	"JOIN":                       (*Server).handleJoin,
	"STATE":                      (*Server).handleState,
	"LEAVE":                      (*Server).handleLeave,
	"SCREEN_START":               (*Server).handleScreenStart,
	"SCREEN_STOP":                (*Server).handleScreenStop,
	"SCREEN_WATCH":               (*Server).handleScreenWatch,
	"SCREEN_UNWATCH":             (*Server).handleScreenUnwatch,
	"SCREEN_KEYREQ":              (*Server).handleScreenKeyReq,
	"DM_CALL":                    (*Server).handleDmCall,
	"DM_ACCEPT_CALL":             (*Server).handleDmCallAccept,
	"DM_REJECT_CALL":             (*Server).handleDmCallReject,
	"DM_CANCEL_CALL":             (*Server).handleDmCallCancel,
	"LIST_SERVERS":               (*Server).handleListServers,
	"CREATE_SERVER":              (*Server).handleCreateServer,
	"JOIN_SERVER":                (*Server).handleJoinServer,
	"UPDATE_SERVER":              (*Server).handleUpdateServer,
	"DELETE_SERVER":              (*Server).handleDeleteServer,
	"LIST_CHANNEL_TYPES":         (*Server).handleListChannelTypes,
	"LIST_CHANNELS":              (*Server).handleListChannels,
	"CREATE_CHANNEL":             (*Server).handleCreateChannel,
	"LIST_MEMBERS":               (*Server).handleListMembers,
	"UPDATE_USERNAME":            (*Server).handleUpdateUsername,
	"UPDATE_PASSWORD":            (*Server).handleChangePassword,
	"DELETE_USER":                (*Server).handleDeleteUser,
	"BAN_USER_FROM_SERVER":       (*Server).handleServerBan,
	"KICK_USER_FROM_SERVER":      (*Server).handleServerKick,
	"UNBAN_USER_FROM_SERVER":     (*Server).handleServerUnban,
	"MAKE_SERVER_ADMIN":          (*Server).handleServerMakeAdmin,
	"MAKE_SERVER_MEMBER":         (*Server).handleServerMakeMember,
	"SEND_FRIENDSHIP_REQUEST":    (*Server).handleSendFriendRequest,
	"FETCH_FRIENDSHIP_REQUEST":   (*Server).handleFetchFriendRequest,
	"ACCEPT_FRIENDSHIP_REQUEST":  (*Server).handleAcceptFriendRequest,
	"DECLINE_FRIENDSHIP_REQUEST": (*Server).handleDeclineFriendRequest,
	"REMOVE_FRIENDSHIP":          (*Server).handleRemoveFriend,
	"BLOCK_FRIENDSHIP":           (*Server).handleBlockFriend,
	"UNBLOCK_FRIENDSHIP":         (*Server).handleUnblockFriend,
	"CANCEL_FRIENDSHIP_REQUEST":  (*Server).handleCancelFriendRequest,
	"SEARCH_USERS":               (*Server).handleSearchUsers,
	"SEND_DIRECT_MESSAGE":        (*Server).handleSendDM,
	"RECIEVE_DIRECT_MESSAGE":     (*Server).handleReceiveDM, // legacy spelling kept for existing clients
	"UPDATE_DIRECT_MESSAGE":      (*Server).handleUpdateDM,
	"DELETE_DIRECT_MESSAGE":      (*Server).handleDeleteDM,
	"MARK_DIRECT_MESSAGE_READ":   (*Server).handleMarkDMRead,
	"LIST_DIRECT_CONVERSATIONS":  (*Server).handleListDMConversations,
	"SEND_SERVER_MESSAGE":        (*Server).handleSendServerMessage,
	"LIST_SERVER_MESSAGES":       (*Server).handleListServerMessages,
	"UPDATE_SERVER_MESSAGE":      (*Server).handleUpdateServerMessage,
	"DELETE_SERVER_MESSAGE":      (*Server).handleDeleteServerMessage,
	"MARK_SERVER_MESSAGE_READ":   (*Server).handleMarkServerMessageRead,
}

// dispatch routes one decoded message, ignoring unknown types (§7 interop
// rule).
func (s *Server) dispatch(c *client, msg protocol.Control) {
	h, ok := handlers[msg.Type]
	if !ok {
		log.Printf("[CONTROL] ignoring unknown type %q from user %d", msg.Type, c.sess.UserID)
		return
	}
	h(s, c, msg)
}
