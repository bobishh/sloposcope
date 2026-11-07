<script>
  import Window from './Window.svelte';

  let {
    coordinatorState = null,
    missionState = null,
    loading = false,
    sending = false,
    error = '',
    x = $bindable(28),
    y = $bindable(28),
    windowWidth = $bindable(460),
    windowHeight = $bindable(580),
    containerWidth = 0,
    containerHeight = 0,
    onfocus = null,
    visible = true,
    onclose = null,
    ondragEnd = null,
    onresizeEnd = null,
    onSendMessage,
    onApprovePlan,
    onLaunchWorkers,
    onAskReport,
    onNudgeLatestWorker,
    onStartThread,
    onResumeThread,
  } = $props();

  let draft = $state('');

  function coordinatorLabel() {
    return coordinatorState?.personaName || 'Coordinator';
  }

  async function send() {
    const content = String(draft || '').trim();
    if (!content || !onSendMessage) return;
    try {
      await onSendMessage(content);
      draft = '';
    } catch {
      // App sets visible error state. Keep draft intact.
    }
  }

  function messages() {
    return Array.isArray(coordinatorState?.messages) ? coordinatorState.messages : [];
  }

  function canLaunchWorkers() {
    return Boolean(
      coordinatorState?.canLaunchWorkers
        || (
          coordinatorState?.missionId
          && missionState?.missionId === coordinatorState?.missionId
          && Number(missionState?.scheduler?.readyCount || 0) > 0
        ),
    );
  }

  function canAskReport() {
    return Boolean(coordinatorState?.missionId && coordinatorState?.canAskReport);
  }

  function canNudgeLatestWorker() {
    return Boolean(coordinatorState?.latestLiveSessionId);
  }

  function threads() {
    return Array.isArray(coordinatorState?.threads) ? coordinatorState.threads : [];
  }

  function hasActiveTask() {
    return Boolean(
      coordinatorState?.missionId
        || coordinatorState?.draftMission?.brief
        || (Array.isArray(coordinatorState?.pendingPlan) && coordinatorState.pendingPlan.length > 0),
    );
  }

  function activeTaskTitle() {
    const title = String(coordinatorState?.title || coordinatorState?.draftMission?.title || '').trim();
    if (title && title !== 'Unscoped mission') return title;
    return 'Mission draft';
  }

  function activeTaskDetail() {
    const pendingPlanCount = Array.isArray(coordinatorState?.pendingPlan) ? coordinatorState.pendingPlan.length : 0;
    const missionObjectiveCount = missionState?.missionId === coordinatorState?.missionId && Array.isArray(missionState?.objectives)
      ? missionState.objectives.length
      : 0;
    const objectiveCount = pendingPlanCount || missionObjectiveCount;
    const pieces = [];
    if (coordinatorState?.canApprovePlan) pieces.push('Plan ready for review');
    else if (canLaunchWorkers()) pieces.push('Ready to launch workers');
    else if (canAskReport()) pieces.push('Mission active');
    else pieces.push('Brief received');
    if (objectiveCount > 0) pieces.push(`${objectiveCount} objective${objectiveCount === 1 ? '' : 's'}`);
    return pieces.join('. ');
  }

  function coordinatorStageLabel() {
    if (coordinatorState?.canApprovePlan) return 'Plan ready';
    if (canLaunchWorkers()) return 'Launch ready';
    if (canAskReport()) return 'Active';
    return 'Listening';
  }

  function messageKindLabel(kind) {
    const labels = {
      wake_brief: 'brief',
      prompt: 'prompt',
      plan: 'plan',
      report: 'report',
      status: 'update',
    };
    return labels[kind] || 'update';
  }

  function messageContent(content) {
    return String(content || '').replace(/\s+in state [a-z0-9_-]+(?=\.)/gi, '');
  }
</script>

<Window
  title="Coordinator"
  className="eyeloss-coordinator-window"
  style="z-index: 1406;"
  {visible}
  {x}
  {y}
  width={windowWidth}
  height={windowHeight}
  minWidth={360}
  minHeight={360}
  {containerWidth}
  {containerHeight}
  onfocus={onfocus}
  onclose={onclose}
  ondragEnd={ondragEnd}
  onresizeEnd={onresizeEnd}
>
  <div class="eyeloss-coordinator-shell" onwheel={(event) => event.stopPropagation()}>
    <header class="eyeloss-coordinator-shell__hero">
      <div class="eyeloss-coordinator-shell__portrait" aria-hidden="true">
        <svg viewBox="0 0 96 96" class="eyeloss-coordinator-shell__portrait-svg">
          <rect x="8" y="8" width="80" height="80" rx="18" />
          <path d="M30 38c2-12 11-19 18-19s16 7 18 19c2 11-2 19-8 24-3 3-7 5-10 5s-7-2-10-5c-6-5-10-13-8-24Z" />
          <path d="M24 79c5-10 14-15 24-15s19 5 24 15" />
          <path d="M30 32c5-8 11-12 18-12 9 0 17 6 20 16-5-2-9-3-13-3-8 0-15 4-25 10-1-4-1-7 0-11Z" />
        </svg>
      </div>
      <div class="eyeloss-coordinator-shell__hero-copy">
        <div class="eyeloss-coordinator-shell__eyebrow">Mission control</div>
        <h2>{coordinatorLabel()}</h2>
        <p>{coordinatorState?.summary || 'Coordinator waiting on mission brief.'}</p>
      </div>
      <div class="eyeloss-coordinator-shell__state">
        <span>{coordinatorStageLabel()}</span>
      </div>
    </header>

    <section class="eyeloss-coordinator-shell__threadbar">
      <label class="eyeloss-coordinator-shell__threadpicker">
        <span>Mission threads</span>
        <select
          class="input-mono"
          aria-label="Mission threads"
          value={coordinatorState?.threadId || ''}
          onchange={(event) => onResumeThread?.(event.currentTarget.value)}
          disabled={loading || sending || threads().length === 0}
        >
          {#each threads() as thread}
            <option value={thread.id}>{thread.title}</option>
          {/each}
        </select>
      </label>
      <button type="button" class="btn" onclick={onStartThread} disabled={loading || sending || !onStartThread}>New thread</button>
    </section>

    {#if error}
      <div class="eyeloss-coordinator-shell__error">{error}</div>
    {/if}

    {#if hasActiveTask()}
      <section class="eyeloss-coordinator-shell__active-task" aria-label="Active task">
        <div>Active task</div>
        <strong>{activeTaskTitle()}</strong>
        <p>{activeTaskDetail()}</p>
      </section>
    {/if}

    <section class="eyeloss-coordinator-shell__actions">
      {#if coordinatorState?.canApprovePlan}
        <button type="button" class="btn btn-primary" onclick={onApprovePlan} disabled={loading || sending || !onApprovePlan}>Approve plan</button>
      {/if}
      {#if canLaunchWorkers()}
        <button type="button" class="btn btn-primary" onclick={onLaunchWorkers} disabled={loading || sending || !onLaunchWorkers}>Launch workers</button>
      {/if}
      {#if canAskReport()}
        <button type="button" class="btn" onclick={onAskReport} disabled={loading || sending || !onAskReport}>Ask report</button>
      {/if}
      {#if canNudgeLatestWorker()}
        <button type="button" class="btn" onclick={onNudgeLatestWorker} disabled={loading || sending || !onNudgeLatestWorker}>Nudge latest worker</button>
      {/if}
    </section>

    <section class="eyeloss-coordinator-shell__transcript" aria-label="Coordinator transcript">
      {#if messages().length === 0}
        <div class="eyeloss-coordinator-shell__empty">Coordinator still booting.</div>
      {:else}
        {#each messages() as message (message.id || `${message.role}-${message.createdAt}`)}
          <article class={`eyeloss-coordinator-shell__message eyeloss-coordinator-shell__message--${message.role || 'coordinator'}`}>
            <div class="eyeloss-coordinator-shell__message-meta">
              <span>{message.role === 'user' ? 'You' : coordinatorLabel()}</span>
              <span>{messageKindLabel(message.kind)}</span>
            </div>
            <pre>{messageContent(message.content)}</pre>
          </article>
        {/each}
      {/if}
    </section>

    <label class="eyeloss-coordinator-shell__composer">
      <span class="sr-only">Message coordinator</span>
      <textarea
        aria-label="Message coordinator"
        class="input-mono"
        rows="3"
        bind:value={draft}
        placeholder="Describe problem or feature. Validation can come later."
        onkeydown={(event) => {
          if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
            event.preventDefault();
            send();
          }
        }}
      ></textarea>
    </label>
    <div class="eyeloss-coordinator-shell__composer-actions">
      <div class="eyeloss-coordinator-shell__hint">Cmd/Ctrl+Enter sends.</div>
      <button type="button" class="btn btn-primary" onclick={send} disabled={loading || sending || !draft.trim()}>Send</button>
    </div>
  </div>
</Window>

<style>
  .eyeloss-coordinator-window :global(.window-content) {
    padding: 0;
    overflow: hidden;
  }

  .eyeloss-coordinator-shell {
    display: grid;
    grid-template-rows: auto auto 1fr auto auto;
    height: 100%;
    background: var(--bg-100);
    color: var(--text);
  }

  .eyeloss-coordinator-shell__hero {
    display: grid;
    grid-template-columns: 72px 1fr auto;
    gap: 14px;
    align-items: center;
    padding: 16px;
    border-bottom: 1px solid var(--bg-300);
    background:
      radial-gradient(circle at top left, color-mix(in srgb, var(--secondary) 18%, transparent), transparent 42%),
      linear-gradient(180deg, color-mix(in srgb, var(--bg-200) 54%, transparent), transparent);
  }

  .eyeloss-coordinator-shell__portrait {
    width: 72px;
    height: 72px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    display: grid;
    place-items: center;
  }

  .eyeloss-coordinator-shell__portrait-svg {
    width: 60px;
    height: 60px;
    fill: none;
    stroke: var(--secondary);
    stroke-width: 4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .eyeloss-coordinator-shell__eyebrow {
    font-size: 11px;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--text-dim);
  }

  .eyeloss-coordinator-shell__hero-copy h2 {
    margin: 2px 0 6px;
    font-size: 22px;
  }

  .eyeloss-coordinator-shell__hero-copy p {
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--text-dim);
  }

  .eyeloss-coordinator-shell__state span {
    display: inline-block;
    padding: 6px 10px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .eyeloss-coordinator-shell__actions,
  .eyeloss-coordinator-shell__composer-actions {
    display: flex;
    gap: 8px;
    align-items: center;
    flex-wrap: wrap;
    padding: 12px 16px;
  }

  .eyeloss-coordinator-shell__actions {
    border-bottom: 1px solid var(--bg-300);
  }

  .eyeloss-coordinator-shell__active-task {
    display: grid;
    gap: 4px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--bg-300);
    background: var(--bg-200);
  }

  .eyeloss-coordinator-shell__active-task div {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-dim);
  }

  .eyeloss-coordinator-shell__active-task strong {
    font-size: 13px;
  }

  .eyeloss-coordinator-shell__active-task p {
    margin: 0;
    font-size: 12px;
    line-height: 1.4;
    color: var(--text-dim);
  }

  .eyeloss-coordinator-shell__threadbar {
    display: flex;
    gap: 10px;
    align-items: end;
    padding: 12px 16px;
    border-bottom: 1px solid var(--bg-300);
  }

  .eyeloss-coordinator-shell__threadpicker {
    display: grid;
    gap: 6px;
    flex: 1;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-dim);
  }

  .eyeloss-coordinator-shell__transcript {
    overflow: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .eyeloss-coordinator-shell__message {
    padding: 12px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
  }

  .eyeloss-coordinator-shell__message--user {
    background: color-mix(in srgb, var(--primary) 12%, var(--bg-100));
    border-color: color-mix(in srgb, var(--primary) 42%, var(--bg-300));
  }

  .eyeloss-coordinator-shell__message-meta {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 8px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-dim);
  }

  .eyeloss-coordinator-shell__message pre {
    margin: 0;
    white-space: pre-wrap;
    font: inherit;
    line-height: 1.45;
  }

  .eyeloss-coordinator-shell__composer {
    padding: 0 16px;
  }

  .eyeloss-coordinator-shell__composer textarea {
    width: 100%;
    min-height: 84px;
    resize: none;
  }

  .eyeloss-coordinator-shell__hint {
    font-size: 11px;
    color: var(--text-dim);
  }

  .eyeloss-coordinator-shell__empty,
  .eyeloss-coordinator-shell__error {
    margin: 0 16px;
    padding: 10px 12px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
  }

  .eyeloss-coordinator-shell__error {
    border-color: color-mix(in srgb, var(--red) 48%, var(--bg-300));
    background: color-mix(in srgb, var(--red) 12%, var(--bg-100));
    color: var(--red);
  }
</style>
