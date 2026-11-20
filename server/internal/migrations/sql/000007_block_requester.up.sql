-- Blocks written before BlockFriendship set requester_id to the blocked user.
-- Those rows were updated in place, so updated_at is later than created_at.
-- Rows written since then are inserted with created_at = updated_at. Swap only
-- the old ones, so the blocker is the requester as the schema says.
UPDATE friendships
SET requester_id = addressee_id, addressee_id = requester_id
WHERE status = 'BLOCKED' AND created_at <> updated_at;
