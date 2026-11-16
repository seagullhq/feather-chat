-- Restore the old direction for rows that the up migration swapped.
UPDATE friendships
SET requester_id = addressee_id, addressee_id = requester_id
WHERE status = 'BLOCKED' AND created_at <> updated_at;
