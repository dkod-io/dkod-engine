#[derive(Debug, Clone, PartialEq)]
pub enum ApprovalDecision {
    Approve,
    Reject,
    Timeout,
}
