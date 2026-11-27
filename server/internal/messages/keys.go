package messages

import (
	"bytes"
	"context"
	"database/sql"
	"errors"
	"time"

	"github.com/seagullhq/feather-chat/server/internal/e2ee"
)

var (
	ErrStaleEpoch    = errors.New("stale epoch")
	ErrEpochConflict = errors.New("epoch conflict")
	ErrBadShare      = errors.New("invalid key share")
	ErrBadMessage    = errors.New("invalid encrypted message")
	ErrDuplicateID   = errors.New("duplicate message id")
	ErrBadRevision   = errors.New("invalid message revision")
)

const (
	// A current epoch with this many messages may be rotated.
	epochMessageLimit = 1000
	maxEpochWraps     = 1000
	maxShares         = 200
	maxEpochPage      = 100
	maxOwnWraps       = 3
	maxMissingKeys    = 200
	rejectInterval    = time.Minute
)

// Address names a conversation as clients do: a DM by its peer, or a channel by
// server and channel (PeerID 0).
type Address struct {
	PeerID   int
	ServerID int
	Channel  ChannelID
}

// desc is the convDesc of the address as userID sees it. It is built from the
// address only, which was checked to name the conversation.
func (a Address) desc(userID int) []byte {
	if a.PeerID != 0 {
		low, high := pair(userID, a.PeerID)
		return e2ee.ConvDesc(e2ee.KindDM, int64(low), int64(high))
	}
	return e2ee.ConvDesc(e2ee.KindChannel, int64(a.ServerID), int64(a.Channel))
}

// ConversationKeys is the state GET_CONVERSATION_KEYS reports.
type ConversationKeys struct {
	ConversationID int64          `json:"conversationId"`
	CurrentEpoch   uint32         `json:"currentEpoch"`
	Stale          bool           `json:"stale"`
	Members        []e2ee.UserKey `json:"members"`
	Epochs         []e2ee.Epoch   `json:"epochs"`
	MoreEpochs     bool           `json:"moreEpochs"`
	Missing        []e2ee.Missing `json:"missing"`
}

// openDM returns the pair's conversation, creating it first if the peer exists.
// An existing conversation stays reachable after the peer is deleted. suffix
// "FOR UPDATE" locks the row.
func openDM(ctx context.Context, q execQueryer, userID, peerID int, suffix string) (int64, error) {
	if userID == peerID {
		return 0, ErrSelfRequest
	}
	low, high := pair(userID, peerID)
	if _, err := q.ExecContext(ctx, `
		INSERT INTO conversations (kind, user_low, user_high)
		SELECT 'direct', $1::bigint, $2::bigint WHERE EXISTS (SELECT 1 FROM users WHERE id = $3::bigint)
		ON CONFLICT (user_low, user_high) DO NOTHING`, low, high, peerID); err != nil {
		return 0, err
	}
	var conv int64
	err := q.QueryRowContext(ctx, `
		SELECT id FROM conversations WHERE user_low = $1 AND user_high = $2 `+suffix, low, high).Scan(&conv)
	if errors.Is(err, sql.ErrNoRows) {
		return 0, ErrNotFound
	}
	return conv, err
}

// openChannel checks access and returns the channel's conversation, creating it
// on first use. suffix "FOR UPDATE OF c" locks the row.
func openChannel(ctx context.Context, q queryer, userID, serverID int, ch ChannelID, suffix string) (int64, error) {
	id, err := resolveServerConversation(ctx, q, userID, serverID, ch)
	if err != nil {
		return 0, err
	}
	var conv int64
	err = q.QueryRowContext(ctx, `
		SELECT c.id FROM conversations c
		JOIN server_channels sc ON sc.id = c.server_channel_id
		JOIN readable_server_channels($3) r ON r.server_channel_id = sc.id
		WHERE c.id = $1 AND r.server_id = $2 AND sc.id = $4 `+suffix,
		int64(id), serverID, userID, int64(ch)).Scan(&conv)
	if errors.Is(err, sql.ErrNoRows) {
		return 0, ErrServerChannelNotFound
	}
	return conv, err
}

// checkDMContact allows key requests for a DM only between users who share a
// server, are friends or already have a message. Others cannot create the
// conversation, nor learn through it whether the peer exists.
func checkDMContact(ctx context.Context, q queryer, userID, peerID int) error {
	if userID == peerID {
		return ErrSelfRequest
	}
	low, high := pair(userID, peerID)
	var ok bool
	if err := q.QueryRowContext(ctx, `
		SELECT EXISTS (
				SELECT 1 FROM server_members a
				JOIN server_members b ON b.server_id = a.server_id
				WHERE a.user_id = $1::bigint AND b.user_id = $2::bigint
				  AND COALESCE(a.banned, FALSE) = FALSE AND COALESCE(b.banned, FALSE) = FALSE)
			OR EXISTS (
				SELECT 1 FROM friendships f WHERE f.status = 'ACCEPTED'
				  AND ((f.requester_id = $1::bigint AND f.addressee_id = $2::bigint)
				    OR (f.requester_id = $2::bigint AND f.addressee_id = $1::bigint)))
			OR EXISTS (
				SELECT 1 FROM conversations c
				JOIN conversation_messages m ON m.conversation_id = c.id
				WHERE c.user_low = $1::bigint AND c.user_high = $2::bigint)`, low, high).Scan(&ok); err != nil {
		return err
	}
	if !ok {
		return ErrNotFound
	}
	return nil
}

// withConversation runs fn in a transaction that holds the conversation lock.
// READ COMMITTED: every statement after the lock sees what its previous holder
// committed.
func (s *ServerStore) withConversation(ctx context.Context, userID int, a Address, fn func(*sql.Tx, int64) error) error {
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: sql.LevelReadCommitted})
	if err != nil {
		return err
	}
	defer tx.Rollback()
	var conv int64
	if a.PeerID != 0 {
		conv, err = openDM(ctx, tx, userID, a.PeerID, "FOR UPDATE")
	} else {
		// The shared lock on the server row, taken first, orders writes against
		// kick, ban and server deletion.
		if _, err := tx.ExecContext(ctx, `SELECT 1 FROM servers WHERE id = $1 FOR SHARE`, a.ServerID); err != nil {
			return err
		}
		conv, err = openChannel(ctx, tx, userID, a.ServerID, a.Channel, "FOR UPDATE OF c")
	}
	if err != nil {
		return err
	}
	if err := fn(tx, conv); err != nil {
		return err
	}
	return tx.Commit()
}

// withKeyRequest is withConversation for the key messages of §4.2.
func (s *ServerStore) withKeyRequest(ctx context.Context, userID int, a Address, fn func(*sql.Tx, int64) error) error {
	if a.PeerID != 0 {
		if err := checkDMContact(ctx, s.db, userID, a.PeerID); err != nil {
			return err
		}
	}
	return s.withConversation(ctx, userID, a, fn)
}

// currentEpoch returns the newest epoch (0 = none) and whether it is stale: a
// recorded recipient is no longer a member or has retired the recorded key, or
// a wrap of its creator was rejected.
func currentEpoch(ctx context.Context, tx *sql.Tx, conv int64) (epoch uint32, stale bool, err error) {
	err = tx.QueryRowContext(ctx, `
		WITH cur AS (
			SELECT COALESCE(max(epoch), 0) AS epoch FROM conversation_epochs WHERE conversation_id = $1
		)
		SELECT cur.epoch, EXISTS (
			SELECT 1 FROM conversation_epochs e
			WHERE e.conversation_id = $1 AND e.epoch = cur.epoch AND e.rejected_by IS NOT NULL
		) OR EXISTS (
			SELECT 1 FROM epoch_recipients r
			WHERE r.conversation_id = $1 AND r.epoch = cur.epoch
			  AND (r.user_id NOT IN (SELECT user_id FROM conversation_member_ids($1))
			       OR NOT EXISTS (SELECT 1 FROM user_keys k
			                      WHERE k.id = r.key_id AND k.user_id = r.user_id AND k.retired_at IS NULL)))
		FROM cur`, conv).Scan(&epoch, &stale)
	return epoch, stale, err
}

// keyedMembers lists the members that have current keys.
func keyedMembers(ctx context.Context, tx *sql.Tx, conv int64) ([]e2ee.UserKey, error) {
	rows, err := tx.QueryContext(ctx, `
		SELECT k.user_id, k.id, k.identity_pub, k.enc_pub, k.key_sig
		FROM conversation_member_ids($1) m
		JOIN user_keys k ON k.user_id = m.user_id AND k.retired_at IS NULL
		ORDER BY k.user_id`, conv)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []e2ee.UserKey{}
	for rows.Next() {
		var k e2ee.UserKey
		if err := rows.Scan(&k.UserID, &k.KeyID, &k.IdentityPub, &k.EncPub, &k.KeySig); err != nil {
			return nil, err
		}
		out = append(out, k)
	}
	return out, rows.Err()
}

func byUser(keys []e2ee.UserKey) map[int]e2ee.UserKey {
	m := make(map[int]e2ee.UserKey, len(keys))
	for _, k := range keys {
		m[k.UserID] = k
	}
	return m
}

// ConversationKeys answers GET_CONVERSATION_KEYS. beforeEpoch 0 asks for the
// newest page. It takes no row lock: one snapshot makes the answer consistent.
func (s *ServerStore) ConversationKeys(ctx context.Context, userID int, a Address, beforeEpoch uint32) (*ConversationKeys, error) {
	out := &ConversationKeys{Epochs: []e2ee.Epoch{}, Missing: []e2ee.Missing{}}
	// The conversation is created outside the snapshot: a concurrent creation
	// would fail a repeatable read transaction.
	var err error
	if a.PeerID != 0 {
		if err = checkDMContact(ctx, s.db, userID, a.PeerID); err != nil {
			return nil, err
		}
		out.ConversationID, err = openDM(ctx, s.db, userID, a.PeerID, "")
	} else {
		out.ConversationID, err = openChannel(ctx, s.db, userID, a.ServerID, a.Channel, "")
	}
	if err != nil {
		return nil, err
	}
	tx, err := s.db.BeginTx(ctx, &sql.TxOptions{Isolation: sql.LevelRepeatableRead, ReadOnly: true})
	if err != nil {
		return nil, err
	}
	defer tx.Rollback()
	conv := out.ConversationID
	if a.PeerID == 0 { // access may have ended since
		var one int
		err := tx.QueryRowContext(ctx, `
			SELECT 1 FROM readable_server_channels($1) WHERE server_channel_id = $2`,
			userID, int64(a.Channel)).Scan(&one)
		if errors.Is(err, sql.ErrNoRows) {
			return nil, ErrServerChannelNotFound
		}
		if err != nil {
			return nil, err
		}
	}
	if out.CurrentEpoch, out.Stale, err = currentEpoch(ctx, tx, conv); err != nil {
		return nil, err
	}
	if out.Members, err = keyedMembers(ctx, tx, conv); err != nil {
		return nil, err
	}
	if err := listEpochs(ctx, tx, userID, int64(beforeEpoch), out); err != nil {
		return nil, err
	}
	if err := listMissing(ctx, tx, userID, out); err != nil {
		return nil, err
	}
	return out, tx.Commit()
}

// listMissing fills the members lacking a wrap for the listed epochs that the
// caller can share. Only the listed page is examined, which bounds the cost.
func listMissing(ctx context.Context, tx *sql.Tx, userID int, out *ConversationKeys) error {
	if len(out.Epochs) == 0 {
		return nil
	}
	rows, err := tx.QueryContext(ctx, `
		WITH held AS (
			SELECT e.epoch FROM conversation_epochs e
			WHERE e.conversation_id = $1 AND e.epoch BETWEEN $4 AND $5 AND EXISTS (
				SELECT 1 FROM epoch_wraps w
				JOIN user_keys mine ON mine.id = w.recipient_key_id AND mine.retired_at IS NULL
				WHERE w.conversation_id = $1 AND w.epoch = e.epoch AND w.recipient_id = $2 AND NOT w.rejected)
		), keyed AS MATERIALIZED (
			SELECT k.user_id, k.id FROM conversation_member_ids($1) m
			JOIN user_keys k ON k.user_id = m.user_id AND k.retired_at IS NULL
		)
		SELECT k.user_id, held.epoch
		FROM held CROSS JOIN keyed k
		WHERE NOT EXISTS (
			SELECT 1 FROM epoch_wraps w
			WHERE w.conversation_id = $1 AND w.epoch = held.epoch AND w.recipient_id = k.user_id
			  AND ((NOT w.rejected AND w.recipient_key_id = k.id) OR (w.rejected AND w.sender_id = $2)))
		ORDER BY held.epoch DESC, k.user_id
		LIMIT $3`, out.ConversationID, userID, maxMissingKeys,
		int64(out.Epochs[len(out.Epochs)-1].Epoch), int64(out.Epochs[0].Epoch))
	if err != nil {
		return err
	}
	defer rows.Close()
	for rows.Next() {
		var m e2ee.Missing
		if err := rows.Scan(&m.UserID, &m.Epoch); err != nil {
			return err
		}
		out.Missing = append(out.Missing, m)
	}
	return rows.Err()
}

// listEpochs fills one newest-first page of epochs with the caller's wraps.
func listEpochs(ctx context.Context, tx *sql.Tx, userID int, before int64, out *ConversationKeys) error {
	rows, err := tx.QueryContext(ctx, `
		SELECT epoch, creator_id, creator_key_id, key_commit, epoch_sig
		FROM conversation_epochs
		WHERE conversation_id = $1 AND ($2::bigint = 0 OR epoch < $2::bigint)
		ORDER BY epoch DESC LIMIT $3`, out.ConversationID, before, maxEpochPage+1)
	if err != nil {
		return err
	}
	defer rows.Close()
	for rows.Next() {
		e := e2ee.Epoch{Wraps: []e2ee.Wrap{}}
		if err := rows.Scan(&e.Epoch, &e.CreatorID, &e.CreatorKeyID, &e.KeyCommit, &e.EpochSig); err != nil {
			return err
		}
		out.Epochs = append(out.Epochs, e)
	}
	if err := rows.Err(); err != nil {
		return err
	}
	if len(out.Epochs) > maxEpochPage {
		out.Epochs, out.MoreEpochs = out.Epochs[:maxEpochPage], true
	}
	if len(out.Epochs) == 0 {
		return nil
	}
	index := make(map[uint32]int, len(out.Epochs))
	for i, e := range out.Epochs {
		index[e.Epoch] = i
	}
	// Per epoch: the creator's wrap by its unique key, then the oldest others,
	// so an epoch with many senders is not read in full.
	const columns = `w.id, w.sender_id, w.sender_key_id, w.recipient_key_id, w.enc, w.ct, w.wrap_sig
		FROM epoch_wraps w
		WHERE w.conversation_id = e.conversation_id AND w.epoch = e.epoch AND w.recipient_id = $2
		  AND NOT w.rejected`
	wraps, err := tx.QueryContext(ctx, `
		SELECT e.epoch, p.sender_id, p.sender_key_id, p.recipient_key_id, p.enc, p.ct, p.wrap_sig
		FROM conversation_epochs e CROSS JOIN LATERAL (
			(SELECT 0 AS rank, `+columns+` AND w.sender_id = e.creator_id)
			UNION ALL
			(SELECT 1 AS rank, `+columns+` AND w.sender_id <> e.creator_id ORDER BY w.id LIMIT $5)
			ORDER BY rank, id LIMIT $5
		) p
		WHERE e.conversation_id = $1 AND e.epoch BETWEEN $3 AND $4
		ORDER BY e.epoch, p.rank, p.id`, out.ConversationID, userID,
		int64(out.Epochs[len(out.Epochs)-1].Epoch), int64(out.Epochs[0].Epoch), maxOwnWraps)
	if err != nil {
		return err
	}
	defer wraps.Close()
	for wraps.Next() {
		var epoch uint32
		var w e2ee.Wrap
		if err := wraps.Scan(&epoch, &w.SenderID, &w.SenderKeyID, &w.RecipientKeyID, &w.Enc, &w.CT, &w.WrapSig); err != nil {
			return err
		}
		if i, ok := index[epoch]; ok {
			out.Epochs[i].Wraps = append(out.Epochs[i].Wraps, w)
		}
	}
	return wraps.Err()
}

// validWrap checks a wrap's lengths, that it targets the recipient's current
// key, and the sender's signature.
func validWrap(conv int64, desc []byte, epoch uint32, sender, recipient e2ee.UserKey, keyCommit []byte, w e2ee.NewWrap) bool {
	if w.RecipientKeyID != recipient.KeyID || len(w.Enc) != e2ee.KeyLen || len(w.CT) != e2ee.CTLen {
		return false
	}
	signed := e2ee.WrapHead(conv, desc, epoch, sender.UserID, sender.KeyID, recipient.UserID, recipient.KeyID,
		recipient.EncPub, keyCommit)
	signed = append(append(signed, w.Enc...), w.CT...)
	return e2ee.Verify(sender.IdentityPub, signed, w.WrapSig)
}

// insertWrap stores a wrap and records its recipient. A duplicate (epoch,
// recipient, sender) is ignored.
func insertWrap(ctx context.Context, tx *sql.Tx, conv int64, epoch uint32, sender e2ee.UserKey, w e2ee.NewWrap) (bool, error) {
	res, err := tx.ExecContext(ctx, `
		INSERT INTO epoch_wraps (conversation_id, epoch, recipient_id, recipient_key_id,
		                         sender_id, sender_key_id, enc, ct, wrap_sig)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
		ON CONFLICT (conversation_id, epoch, recipient_id, sender_id) DO NOTHING`,
		conv, int64(epoch), w.RecipientID, w.RecipientKeyID, sender.UserID, sender.KeyID, w.Enc, w.CT, w.WrapSig)
	if err != nil {
		return false, err
	}
	n, err := res.RowsAffected()
	if err != nil || n == 0 {
		return false, err
	}
	_, err = tx.ExecContext(ctx, `
		INSERT INTO epoch_recipients (conversation_id, epoch, user_id, key_id)
		VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING`, conv, int64(epoch), w.RecipientID, w.RecipientKeyID)
	return true, err
}

// NewEpoch is the CREATE_EPOCH payload.
type NewEpoch struct {
	Epoch        uint32
	KeyCommit    []byte
	CreatorKeyID int64
	EpochSig     []byte
	Wraps        []e2ee.NewWrap
}

// CreateEpoch stores the next epoch if every rule of §4.2 holds.
func (s *ServerStore) CreateEpoch(ctx context.Context, userID int, a Address, e NewEpoch) (conv int64, err error) {
	err = s.withKeyRequest(ctx, userID, a, func(tx *sql.Tx, id int64) error {
		conv = id
		current, stale, err := currentEpoch(ctx, tx, conv)
		if err != nil {
			return err
		}
		if e.Epoch != current+1 || e.Epoch > 1<<31-1 {
			return ErrEpochConflict
		}
		if current != 0 && !stale {
			var n int
			if err := tx.QueryRowContext(ctx, `
				SELECT count(*) FROM (SELECT 1 FROM conversation_messages
				WHERE conversation_id = $1 AND e2ee_epoch = $2 LIMIT $3) m`,
				conv, int64(current), epochMessageLimit).Scan(&n); err != nil {
				return err
			}
			if n < epochMessageLimit {
				return ErrEpochConflict
			}
		}
		list, err := keyedMembers(ctx, tx, conv)
		if err != nil {
			return err
		}
		members := byUser(list)
		creator, ok := members[userID]
		if !ok || creator.KeyID != e.CreatorKeyID || len(e.KeyCommit) != e2ee.KeyLen ||
			len(e.Wraps) != len(members) || len(e.Wraps) > maxEpochWraps ||
			!e2ee.Verify(creator.IdentityPub,
				e2ee.EpochRecord(conv, a.desc(userID), e.Epoch, userID, creator.KeyID, e.KeyCommit), e.EpochSig) {
			return ErrEpochConflict
		}
		// Equal counts, only members and no repeats: exactly the keyed members.
		seen := make(map[int]bool, len(e.Wraps))
		for _, w := range e.Wraps {
			recipient, ok := members[w.RecipientID]
			if !ok || seen[w.RecipientID] || !validWrap(conv, a.desc(userID), e.Epoch, creator, recipient, e.KeyCommit, w) {
				return ErrEpochConflict
			}
			seen[w.RecipientID] = true
		}
		if _, err := tx.ExecContext(ctx, `
			INSERT INTO conversation_epochs (conversation_id, epoch, creator_id, creator_key_id, key_commit, epoch_sig)
			VALUES ($1, $2, $3, $4, $5, $6)`,
			conv, int64(e.Epoch), userID, creator.KeyID, e.KeyCommit, e.EpochSig); err != nil {
			return err
		}
		for _, w := range e.Wraps {
			if _, err := insertWrap(ctx, tx, conv, e.Epoch, creator, w); err != nil {
				return err
			}
		}
		return nil
	})
	return conv, err
}

// ShareKeys stores wraps of existing epochs, all or none, and returns the
// recipients that got a new one.
func (s *ServerStore) ShareKeys(ctx context.Context, userID int, a Address, senderKeyID int64, shares []e2ee.NewWrap) (conv int64, recipients []int, err error) {
	if len(shares) == 0 || len(shares) > maxShares {
		return 0, nil, ErrBadShare
	}
	err = s.withKeyRequest(ctx, userID, a, func(tx *sql.Tx, id int64) error {
		conv = id
		list, err := keyedMembers(ctx, tx, conv)
		if err != nil {
			return err
		}
		members := byUser(list)
		sender, ok := members[userID]
		if !ok || sender.KeyID != senderKeyID {
			return ErrBadShare
		}
		commits := map[uint32][]byte{} // epochs the caller holds a wrap for
		added := map[int]bool{}
		for _, w := range shares {
			commit, known := commits[w.Epoch]
			if !known {
				err := tx.QueryRowContext(ctx, `
					SELECT e.key_commit FROM conversation_epochs e
					WHERE e.conversation_id = $1 AND e.epoch = $2 AND EXISTS (
						SELECT 1 FROM epoch_wraps w
						WHERE w.conversation_id = e.conversation_id AND w.epoch = e.epoch
						  AND w.recipient_id = $3 AND w.recipient_key_id = $4 AND NOT w.rejected)`,
					conv, int64(w.Epoch), userID, senderKeyID).Scan(&commit)
				if errors.Is(err, sql.ErrNoRows) {
					return ErrBadShare
				}
				if err != nil {
					return err
				}
				commits[w.Epoch] = commit
			}
			recipient, ok := members[w.RecipientID]
			if !ok || !validWrap(conv, a.desc(userID), w.Epoch, sender, recipient, commit, w) {
				return ErrBadShare
			}
			inserted, err := insertWrap(ctx, tx, conv, w.Epoch, sender, w)
			if err != nil {
				return err
			}
			if inserted && !added[w.RecipientID] {
				added[w.RecipientID] = true
				recipients = append(recipients, w.RecipientID)
			}
		}
		return nil
	})
	if err != nil {
		return 0, nil, err
	}
	return conv, recipients, nil
}

// RejectWrap marks the wrap sender made for the caller as unusable. A rejected
// wrap of the current epoch's creator makes that epoch stale, at most once per
// rejectInterval for each user and conversation.
func (s *ServerStore) RejectWrap(ctx context.Context, userID int, a Address, epoch uint32, senderID int) (conv int64, err error) {
	err = s.withKeyRequest(ctx, userID, a, func(tx *sql.Tx, id int64) error {
		conv = id
		var rejected bool
		err := tx.QueryRowContext(ctx, `
			SELECT rejected FROM epoch_wraps
			WHERE conversation_id = $1 AND epoch = $2 AND recipient_id = $3 AND sender_id = $4`,
			conv, int64(epoch), userID, senderID).Scan(&rejected)
		if errors.Is(err, sql.ErrNoRows) {
			return ErrBadShare
		}
		if err != nil || rejected { // a repeat changes nothing
			return err
		}
		if _, err := tx.ExecContext(ctx, `
			UPDATE epoch_wraps SET rejected = TRUE
			WHERE conversation_id = $1 AND epoch = $2 AND recipient_id = $3 AND sender_id = $4`,
			conv, int64(epoch), userID, senderID); err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, `
			UPDATE conversation_epochs e SET rejected_by = $3, rejected_at = now()
			WHERE e.conversation_id = $1 AND e.epoch = $2 AND e.creator_id = $4 AND e.rejected_by IS NULL
			  AND e.epoch = (SELECT max(epoch) FROM conversation_epochs WHERE conversation_id = $1)
			  AND NOT EXISTS (
				SELECT 1 FROM conversation_epochs r
				WHERE r.conversation_id = $1 AND r.rejected_by = $3
				  AND r.rejected_at > now() - make_interval(secs => $5))`,
			conv, int64(epoch), userID, senderID, rejectInterval.Seconds())
		return err
	})
	return conv, err
}

// checkMessage applies the server checks of §5 under the conversation lock.
func checkMessage(ctx context.Context, tx *sql.Tx, conv int64, senderID int, m *e2ee.Message) error {
	if m == nil || !m.ValidShape() {
		return ErrBadMessage
	}
	var identity []byte
	err := tx.QueryRowContext(ctx, `
		SELECT identity_pub FROM user_keys
		WHERE id = $1 AND user_id = $2 AND retired_at IS NULL`, m.SenderKeyID, senderID).Scan(&identity)
	if errors.Is(err, sql.ErrNoRows) {
		return ErrBadMessage
	}
	if err != nil {
		return err
	}
	if !e2ee.Verify(identity, m.SignedBytes(conv, senderID), m.MsgSig) {
		return ErrBadMessage
	}
	current, stale, err := currentEpoch(ctx, tx, conv)
	if err != nil {
		return err
	}
	if current == 0 || m.Epoch != current || stale {
		return ErrStaleEpoch
	}
	return nil
}

// checkNew is checkMessage for a message not stored yet. A used msgId fails on
// the unique index.
func checkNew(ctx context.Context, tx *sql.Tx, conv int64, senderID int, m *e2ee.Message) error {
	if err := checkMessage(ctx, tx, conv, senderID, m); err != nil {
		return err
	}
	if m.Revision != 0 {
		return ErrBadRevision
	}
	return nil
}

// e2eeColumns are read into e2eeRow, in this order.
const e2eeColumns = `m.e2ee_epoch, m.e2ee_msg_id, m.e2ee_revision, m.e2ee_sender_key, m.e2ee_box, m.e2ee_sig`

type e2eeRow struct {
	epoch, revision, key sql.NullInt64
	id, box, sig         []byte
}

func (r *e2eeRow) dest() []any {
	return []any{&r.epoch, &r.id, &r.revision, &r.key, &r.box, &r.sig}
}

// message is nil for plaintext and deleted rows.
func (r *e2eeRow) message() *e2ee.Message {
	if r.box == nil {
		return nil
	}
	return &e2ee.Message{Epoch: uint32(r.epoch.Int64), MsgID: r.id, Revision: uint32(r.revision.Int64),
		SenderKeyID: r.key.Int64, Box: r.box, MsgSig: r.sig}
}

// checkEdit requires the same msgId and the next revision. Plaintext rows have
// no msgId and cannot be edited.
func checkEdit(old e2eeRow, m *e2ee.Message) error {
	if old.box == nil || !bytes.Equal(old.id, m.MsgID) || int64(m.Revision) != old.revision.Int64+1 {
		return ErrBadRevision
	}
	return nil
}
