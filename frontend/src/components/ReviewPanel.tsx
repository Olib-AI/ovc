import { useState } from 'react';
import {
  CheckCircle2,
  XCircle,
  MessageSquare,
  Pencil,
  Trash2,
  ShieldCheck,
  Send,
  Loader2,
  ChevronDown,
  ChevronRight,
} from 'lucide-react';
import {
  useListReviews,
  useCreateReview,
  useListComments,
  useCreateComment,
  useUpdateComment,
  useDeleteComment,
} from '../hooks/useRepo.ts';
import { useToast } from '../contexts/ToastContext.tsx';
import LoadingSpinner from './LoadingSpinner.tsx';
import type { PrReview, PrComment, ReviewState } from '../api/types.ts';

interface ReviewPanelProps {
  repoId: string;
  prNumber: number;
  prState: string;
}

const REVIEW_STATE_BADGE: Record<ReviewState, string> = {
  approved: 'bg-green-500/15 text-green-400',
  changes_requested: 'bg-orange-500/15 text-orange-400',
  commented: 'bg-gray-500/15 text-gray-400',
};

const REVIEW_STATE_LABEL: Record<ReviewState, string> = {
  approved: 'Approved',
  changes_requested: 'Changes Requested',
  commented: 'Commented',
};

const REVIEW_STATE_ICON: Record<ReviewState, typeof CheckCircle2> = {
  approved: CheckCircle2,
  changes_requested: XCircle,
  commented: MessageSquare,
};

function formatTimestamp(iso: string): string {
  return new Date(iso).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function ReviewItem({ review }: { review: PrReview }) {
  const Icon = REVIEW_STATE_ICON[review.state];
  return (
    <div className="flex items-start gap-2 py-2">
      <Icon
        size={14}
        className={`mt-0.5 flex-shrink-0 ${
          review.state === 'approved'
            ? 'text-green-400'
            : review.state === 'changes_requested'
              ? 'text-orange-400'
              : 'text-gray-400'
        }`}
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5 flex-wrap">
          <span className="text-[11px] font-semibold text-text-primary">{review.author_identity ?? review.author}</span>
          {review.verified && <span title="Verified"><ShieldCheck size={11} className="text-green-400" /></span>}
          <span className={`${REVIEW_STATE_BADGE[review.state]} rounded px-1.5 py-px text-[10px] font-medium`}>
            {REVIEW_STATE_LABEL[review.state]}
          </span>
          <span className="text-[10px] text-text-muted">{formatTimestamp(review.created_at)}</span>
        </div>
        {review.body && (
          <p className="mt-1 text-[11px] leading-relaxed text-text-secondary whitespace-pre-wrap">{review.body}</p>
        )}
      </div>
    </div>
  );
}

function CommentItem({
  comment,
  prNumber,
  repoId,
}: {
  comment: PrComment;
  prNumber: number;
  repoId: string;
}) {
  const toast = useToast();
  const updateComment = useUpdateComment(repoId);
  const deleteComment = useDeleteComment(repoId);
  const [editing, setEditing] = useState(false);
  const [editBody, setEditBody] = useState(comment.body);
  const isOwn = comment.author === 'local-user';

  function handleSaveEdit() {
    const trimmed = editBody.trim();
    if (!trimmed || trimmed === comment.body) {
      setEditing(false);
      setEditBody(comment.body);
      return;
    }
    updateComment.mutate(
      { prNumber, commentId: comment.id, body: trimmed },
      {
        onSuccess: () => { setEditing(false); toast.success('Comment updated'); },
        onError: () => toast.error('Failed to update comment'),
      },
    );
  }

  function handleDelete() {
    deleteComment.mutate(
      { prNumber, commentId: comment.id },
      {
        onSuccess: () => toast.success('Comment deleted'),
        onError: () => toast.error('Failed to delete comment'),
      },
    );
  }

  return (
    <div className="flex items-start gap-2 py-2">
      <MessageSquare size={13} className="mt-0.5 flex-shrink-0 text-text-muted" />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <span className="text-[11px] font-semibold text-text-primary">{comment.author_identity ?? comment.author}</span>
          <span className="text-[10px] text-text-muted">{formatTimestamp(comment.created_at)}</span>
          {comment.updated_at !== comment.created_at && (
            <span className="text-[10px] text-text-muted italic">(edited)</span>
          )}
          {isOwn && !editing && (
            <div className="ml-auto flex items-center gap-0.5">
              <button onClick={() => { setEditBody(comment.body); setEditing(true); }} className="rounded p-0.5 text-text-muted hover:text-text-primary" aria-label="Edit">
                <Pencil size={11} />
              </button>
              <button onClick={handleDelete} disabled={deleteComment.isPending} className="rounded p-0.5 text-text-muted hover:text-red-400 disabled:opacity-50" aria-label="Delete">
                <Trash2 size={11} />
              </button>
            </div>
          )}
        </div>
        {editing ? (
          <div className="mt-1.5 space-y-1.5">
            <textarea
              value={editBody}
              onChange={(e) => setEditBody(e.target.value)}
              rows={2}
              className="w-full resize-none rounded border border-border bg-navy-950 px-2 py-1 text-[11px] text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
            />
            <div className="flex gap-1.5">
              <button onClick={handleSaveEdit} disabled={updateComment.isPending} className="rounded bg-accent px-2 py-0.5 text-[11px] font-medium text-navy-950 hover:bg-accent-light disabled:opacity-50">
                {updateComment.isPending ? 'Saving...' : 'Save'}
              </button>
              <button onClick={() => { setEditing(false); setEditBody(comment.body); }} className="rounded px-2 py-0.5 text-[11px] text-text-muted hover:text-text-primary">
                Cancel
              </button>
            </div>
          </div>
        ) : (
          comment.body && (
            <p className="mt-1 text-[11px] leading-relaxed text-text-secondary whitespace-pre-wrap">{comment.body}</p>
          )
        )}
      </div>
    </div>
  );
}

function ReviewPanel({ repoId, prNumber, prState }: ReviewPanelProps) {
  const toast = useToast();
  const { data: reviews, isLoading: reviewsLoading } = useListReviews(repoId, prNumber);
  const { data: comments, isLoading: commentsLoading } = useListComments(repoId, prNumber);
  const createReview = useCreateReview(repoId);
  const createComment = useCreateComment(repoId);

  const [expanded, setExpanded] = useState(true);
  const [reviewBody, setReviewBody] = useState('');
  const [commentBody, setCommentBody] = useState('');

  const reviewCount = reviews?.length ?? 0;
  const commentCount = comments?.length ?? 0;
  const totalCount = reviewCount + commentCount;
  const isLoading = reviewsLoading || commentsLoading;

  function handleSubmitReview(state: ReviewState) {
    createReview.mutate(
      { prNumber, payload: { state, body: reviewBody.trim() } },
      {
        onSuccess: () => {
          setReviewBody('');
          toast.success(
            state === 'approved' ? 'PR approved' : state === 'changes_requested' ? 'Changes requested' : 'Review submitted',
          );
        },
        onError: () => toast.error('Failed to submit review'),
      },
    );
  }

  function handleSubmitComment() {
    const body = commentBody.trim();
    if (!body) return;
    createComment.mutate(
      { prNumber, payload: { body } },
      {
        onSuccess: () => { setCommentBody(''); toast.success('Comment added'); },
        onError: () => toast.error('Failed to add comment'),
      },
    );
  }

  return (
    <div className="flex-shrink-0 border-b border-border">
      {/* Collapsible header */}
      <button
        onClick={() => setExpanded((v) => !v)}
        className="flex w-full items-center gap-2 bg-navy-800/30 px-6 py-2.5 text-left hover:bg-navy-800/50 transition-colors"
      >
        {expanded ? <ChevronDown size={13} className="text-text-muted" /> : <ChevronRight size={13} className="text-text-muted" />}
        <span className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
          Reviews &amp; Comments
        </span>
        {totalCount > 0 && (
          <span className="rounded-full bg-accent/15 px-1.5 py-px text-[10px] font-semibold text-accent">
            {totalCount}
          </span>
        )}
        {isLoading && <Loader2 size={12} className="animate-spin text-text-muted" />}
      </button>

      {expanded && (
        <div className="max-h-80 overflow-y-auto px-6 py-3">
          {isLoading ? (
            <div className="flex items-center justify-center py-4">
              <LoadingSpinner size={16} />
            </div>
          ) : (
            <>
              {/* Existing reviews */}
              {reviewCount > 0 && (
                <div className="mb-3">
                  {reviews!.map((review) => (
                    <ReviewItem key={review.id} review={review} />
                  ))}
                </div>
              )}

              {/* Existing comments */}
              {commentCount > 0 && (
                <div className={reviewCount > 0 ? 'border-t border-border/50 pt-2 mb-3' : 'mb-3'}>
                  {comments!.map((comment) => (
                    <CommentItem key={comment.id} comment={comment} prNumber={prNumber} repoId={repoId} />
                  ))}
                </div>
              )}

              {totalCount === 0 && (
                <p className="py-2 text-[11px] italic text-text-muted">No reviews or comments yet.</p>
              )}

              {/* Submit review (open PRs only) */}
              {prState === 'open' && (
                <div className="border-t border-border/50 pt-3 mt-1">
                  <textarea
                    value={reviewBody}
                    onChange={(e) => setReviewBody(e.target.value)}
                    placeholder="Leave a review..."
                    rows={2}
                    className="w-full resize-none rounded border border-border bg-navy-950 px-2 py-1.5 text-[11px] text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
                  />
                  <div className="mt-2 flex items-center gap-1.5">
                    <button
                      onClick={() => handleSubmitReview('approved')}
                      disabled={createReview.isPending}
                      className="flex items-center gap-1 rounded bg-green-500/15 px-2.5 py-1 text-[11px] font-medium text-green-400 hover:bg-green-500/25 disabled:opacity-50"
                    >
                      {createReview.isPending ? <Loader2 size={11} className="animate-spin" /> : <CheckCircle2 size={11} />}
                      Approve
                    </button>
                    <button
                      onClick={() => handleSubmitReview('changes_requested')}
                      disabled={createReview.isPending}
                      className="flex items-center gap-1 rounded bg-orange-500/15 px-2.5 py-1 text-[11px] font-medium text-orange-400 hover:bg-orange-500/25 disabled:opacity-50"
                    >
                      {createReview.isPending ? <Loader2 size={11} className="animate-spin" /> : <XCircle size={11} />}
                      Request Changes
                    </button>
                    <button
                      onClick={() => handleSubmitReview('commented')}
                      disabled={createReview.isPending}
                      className="flex items-center gap-1 rounded bg-gray-500/15 px-2.5 py-1 text-[11px] font-medium text-gray-400 hover:bg-gray-500/25 disabled:opacity-50"
                    >
                      {createReview.isPending ? <Loader2 size={11} className="animate-spin" /> : <MessageSquare size={11} />}
                      Comment
                    </button>
                  </div>
                </div>
              )}

              {/* Add comment */}
              <div className="border-t border-border/50 pt-3 mt-2">
                <div className="flex gap-2">
                  <textarea
                    value={commentBody}
                    onChange={(e) => setCommentBody(e.target.value)}
                    placeholder="Write a comment..."
                    rows={1}
                    className="flex-1 resize-none rounded border border-border bg-navy-950 px-2 py-1.5 text-[11px] text-text-primary placeholder-text-muted focus:border-accent focus:outline-none"
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) handleSubmitComment();
                    }}
                  />
                  <button
                    onClick={handleSubmitComment}
                    disabled={createComment.isPending || !commentBody.trim()}
                    className="flex-shrink-0 self-end flex items-center gap-1 rounded bg-accent px-2.5 py-1.5 text-[11px] font-semibold text-navy-950 hover:bg-accent-light disabled:opacity-50"
                  >
                    {createComment.isPending ? <Loader2 size={11} className="animate-spin" /> : <Send size={11} />}
                    Comment
                  </button>
                </div>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

export default ReviewPanel;
