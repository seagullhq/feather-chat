-- A finished 1:1 call is logged in the pair's conversation as a message that
-- carries the call details in one JSON column, so both sides see the same
-- history after a reload (same idea as Signal's call-history message type):
--   {"outcome": "ended|rejected|cancelled|missed|offline", "seconds": 201}
-- A row with `call` set is an event, not text: its author cannot edit or
-- delete it. `content` keeps a readable fallback for clients that do not know
-- the event. New call details (video, participants) go into the JSON without
-- a migration.
ALTER TABLE conversation_messages
    ADD COLUMN call JSONB,
    ADD CONSTRAINT conversation_messages_call_check
        CHECK (call IS NULL
               OR (jsonb_typeof(call) = 'object'
                   AND call->>'outcome' IN ('ended', 'rejected', 'cancelled', 'missed', 'offline')));
