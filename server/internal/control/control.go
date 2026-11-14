// Package control implements the TCP control plane: it accepts client
// connections, decodes §7 control messages, and routes each type to its
// handler.
package control

import (
	"log"
	"net"

	"github.com/seagullhq/feather-chat/server/internal/accounts"
	"github.com/seagullhq/feather-chat/server/internal/friends"
	"github.com/seagullhq/feather-chat/server/internal/protocol"
	"github.com/seagullhq/feather-chat/server/internal/room"
	"github.com/seagullhq/feather-chat/server/internal/servers"
)

// Server is the control plane: the room state, the persistence stores, and
// one accept loop feeding per-connection goroutines.
type Server struct {
	rooms   *room.Manager
	users   *accounts.Store
	servers *servers.ServerStore
	friends *friends.ServerStore
}

// New wires a control server around the stores and the room state.
func New(users *accounts.Store, serverStore *servers.ServerStore, friendStore *friends.ServerStore, rooms *room.Manager) *Server {
	return &Server{rooms: rooms, users: users, servers: serverStore, friends: friendStore}
}

// Serve accepts and dispatches control connections until the listener stops.
func (s *Server) Serve(l net.Listener) {
	for {
		conn, err := l.Accept()
		if err != nil {
			log.Println(err)
			return
		}
		go s.handleConn(conn)
	}
}

// handlers routes every control message type to its handler. Unknown types
// fall through silently, per the §7 interop rule.
var handlers = map[string]func(*Server, *client, protocol.Control){
	"REGISTER":                 (*Server).handleRegister,
	"LOGIN":                    (*Server).handleLogin,
	"HELLO":                    (*Server).handleHello,
	"JOIN":                     (*Server).handleJoin,
	"STATE":                    (*Server).handleState,
	"LEAVE":                    (*Server).handleLeave,
	"LIST_SERVERS":             (*Server).handleListServers,
	"CREATE_SERVER":            (*Server).handleCreateServer,
	"JOIN_SERVER":              (*Server).handleJoinServer,
	"UPDATE_SERVER":            (*Server).handleUpdateServer,
	"DELETE_SERVER":            (*Server).handleDeleteServer,
	"LIST_CHANNEL_TYPES":       (*Server).handleListChannelTypes,
	"LIST_CHANNELS":            (*Server).handleListChannels,
	"CREATE_CHANNEL":           (*Server).handleCreateChannel,
	"LIST_MEMBERS":             (*Server).handleListMembers,
	"UPDATE_USERNAME":          (*Server).handleUpdateUsername,
	"UPDATE_PASSWORD":          (*Server).handleChangePassword,
	"DELETE_USER":              (*Server).handleDeleteUser,
	"BAN_USER_FROM_SERVER":     (*Server).handleServerBan,
	"KICK_USER_FROM_SERVER":    (*Server).handleServerKick,
	"UNBAN_USER_FROM_SERVER":   (*Server).handleServerUnban,
	"MAKE_SERVER_ADMIN":        (*Server).handleServerMakeAdmin,
	"MAKE_SERVER_MEMBER":       (*Server).handleServerMakeMember,
	"SEND_FRIENDSHIP_REQUEST":  (*Server).handleSendFriendRequest,
	"FETCH_FRIENDSHIP_REQUEST": (*Server).handleFetchFriendRequest,
	"SEARCH_USERS":             (*Server).handleSearchUsers,
}

// dispatch routes one decoded message, ignoring unknown types (§7 interop
// rule).
func (s *Server) dispatch(c *client, msg protocol.Control) {
	if h, ok := handlers[msg.Type]; ok {
		h(s, c, msg)
	}
}
