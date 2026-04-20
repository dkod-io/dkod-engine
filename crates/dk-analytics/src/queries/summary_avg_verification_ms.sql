-- Arithmetic mean verification-step duration in milliseconds since {since}.
--
-- Params:
--   since: DateTime64(3) lower bound on `created_at`.
SELECT toString(round(avg(duration_ms)))
FROM verification_runs
WHERE created_at >= {since:DateTime64(3)}
