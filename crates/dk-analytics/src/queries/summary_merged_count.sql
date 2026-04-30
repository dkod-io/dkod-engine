-- Number of changesets that reached the `merged` state since {since}.
--
-- Params:
--   since: DateTime64(3) lower bound on `transition_at`.
SELECT toString(count())
FROM changeset_lifecycle
WHERE state = 'merged'
  AND transition_at >= {since:DateTime64(3)}
