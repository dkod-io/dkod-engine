-- Review verdict distribution since {since}.
--
-- Each row is of the form `"verdict:count"` which the caller pretty-prints.
--
-- Params:
--   since: DateTime64(3) lower bound on `created_at`.
SELECT concat(verdict, ':', toString(count()))
FROM review_results
WHERE created_at >= {since:DateTime64(3)}
GROUP BY verdict
ORDER BY verdict
