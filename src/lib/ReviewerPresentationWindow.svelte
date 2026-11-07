<script>
  import Window from './Window.svelte';
  import GeneratedElmSurface from './GeneratedElmSurface.svelte';

  let {
    graph = { nodes: [], edges: [] },
    graphMeta = null,
    reviewTarget = null,
    getFileSource = null,
    getReviewerTargetContext = null,
    generateReviewerElm = null,
    compileReviewerElm = null,
    x = $bindable(506),
    y = $bindable(24),
    windowWidth = $bindable(620),
    windowHeight = $bindable(640),
    containerWidth = 0,
    containerHeight = 0,
    visible = true,
    onclose = null,
    onfocus = null,
    ondragEnd = null,
    onresizeEnd = null,
    onReviewTargetChange = null,
  } = $props();

  let question = $state('');
  let status = $state('idle');
  let error = $state('');
  let bundleJs = $state('');

  const nodes = $derived(Array.isArray(graph?.nodes) ? graph.nodes : []);
  const edges = $derived(Array.isArray(graph?.edges) ? graph.edges : []);
  const activeReviewTarget = $derived.by(() => normalizeReviewTarget(reviewTarget));

  function createDefaultReviewTarget() {
    return {
      mode: 'working_copy',
      since: '@',
      base: '',
      head: '',
      includeWorkingCopy: true,
      label: 'Working copy',
    };
  }

  function normalizeReviewTarget(next, fallback = createDefaultReviewTarget()) {
    const mode = String(next?.mode || fallback.mode || 'working_copy').trim() || 'working_copy';
    const includeWorkingCopy =
      next?.includeWorkingCopy === undefined
        ? Boolean(fallback.includeWorkingCopy)
        : Boolean(next.includeWorkingCopy);

    const normalized = {
      ...fallback,
      ...(next || {}),
      mode,
      since: String(next?.since ?? fallback.since ?? '@').trim() || '@',
      base: String(next?.base ?? fallback.base ?? '').trim(),
      head: String(next?.head ?? fallback.head ?? '').trim(),
      includeWorkingCopy,
      label: String(next?.label ?? fallback.label ?? '').trim(),
    };

    if (!normalized.label) {
      if (mode === 'range') {
        const base = normalized.base || '(base)';
        const head = normalized.head || '(head)';
        normalized.label = `${base}..${head}${normalized.includeWorkingCopy ? ' + wc' : ''}`;
      } else if (mode === 'since') {
        normalized.label = `${normalized.since}${normalized.includeWorkingCopy ? ' + wc' : ''}`;
      } else {
        normalized.label = normalized.includeWorkingCopy ? 'Working copy' : 'Repository';
      }
    }

    return normalized;
  }

  function formatReviewTargetSummary(target) {
    const normalized = normalizeReviewTarget(target);
    if (normalized.mode === 'range') {
      const base = normalized.base || '(base)';
      const head = normalized.head || '(head)';
      return `${base}..${head}${normalized.includeWorkingCopy ? ' + working copy' : ''}`;
    }
    if (normalized.mode === 'since') {
      return `${normalized.since}${normalized.includeWorkingCopy ? ' + working copy' : ''}`;
    }
    return normalized.includeWorkingCopy ? 'Working copy only' : 'Repository only';
  }

  function updateReviewTarget(patch) {
    const current = normalizeReviewTarget(reviewTarget);
    const next = normalizeReviewTarget({ ...current, ...patch }, current);
    onReviewTargetChange?.(next);
  }

  function elmString(value) {
    return JSON.stringify(String(value ?? ''))
      .replaceAll('\\u2028', '\\u{2028}')
      .replaceAll('\\u2029', '\\u{2029}');
  }

  function normalizeFilePath(path) {
    return String(path || '').replaceAll('\\', '/').replace(/^\.\/+/, '');
  }

  function nodeFile(node) {
    return normalizeFilePath(node?.file || node?.id || '');
  }

  function sortReviewNodes(items) {
    return [...items].sort((left, right) => {
      const leftHot = left?.change_status === 'modified' || left?.change_status === 'added' ? 0 : 1;
      const rightHot = right?.change_status === 'modified' || right?.change_status === 'added' ? 0 : 1;
      if (leftHot !== rightHot) return leftHot - rightHot;
      return nodeFile(left).localeCompare(nodeFile(right));
    });
  }

  function snippetFromSource(source) {
    return String(source || '').split('\n').slice(0, 14).join('\n').trim();
  }

  async function loadEvidence(reviewNodes) {
    const snippets = [];
    for (const node of reviewNodes.slice(0, 3)) {
      const file = nodeFile(node);
      if (!file) continue;
      let excerpt = '';
      if (getFileSource) {
        try {
          excerpt = snippetFromSource(await getFileSource(file));
        } catch (_error) {
          excerpt = '';
        }
      }
      snippets.push({
        title: String(node?.id || file),
        file,
        excerpt: excerpt || `${node?.type || 'entity'} ${node?.id || file}\n-- source unavailable`,
      });
    }
    return snippets;
  }

  function elmList(items) {
    return `[ ${items.join(', ')} ]`;
  }

  function generatedElmSource(kind, prompt, reviewNodes, snippets, target, context = null) {
    const changed = reviewNodes.filter((node) => node?.change_status === 'modified' || node?.change_status === 'added');
    const firstFile = snippets[0]?.file || nodeFile(reviewNodes[0]) || 'unknown';
    const targetSummary = formatReviewTargetSummary(target);
    const title = kind === 'question' ? 'Review answer' : 'Task review';
    const lead = kind === 'question'
      ? `Question: ${prompt}\nTarget: ${targetSummary}`
      : `What changed, why it likely changed, and where reviewer should look first.\nTarget: ${targetSummary}`;
    const statRows = [
      ['visible nodes', nodes.length],
      ['visible edges', edges.length],
      ['canonical entities', graphMeta?.canonicalEntityCount || nodes.length],
      ['canonical edges', graphMeta?.canonicalEdgeCount || edges.length],
      ['changed files', context?.changedFilesTotal ?? 0],
      ['target mode', target?.mode || 'working_copy'],
      ['review target', targetSummary],
    ];
    const fileRows = snippets.map((snippet) => [snippet.title, snippet.file]);
    const codeRows = snippets.map((snippet) => [snippet.title, snippet.file, snippet.excerpt]);

    return `module Main exposing (main)

import Browser
import Html exposing (Html, button, code, div, h1, h2, h3, li, ol, p, pre, section, span, strong, text)
import Html.Attributes exposing (class, style)
import Html.Events exposing (onClick)
import Svg exposing (circle, line, svg)
import Svg.Attributes as SA


type alias Model =
    { slide : Int }


type Msg
    = Select Int


main : Program () Model Msg
main =
    Browser.sandbox { init = { slide = 0 }, update = update, view = view }


update : Msg -> Model -> Model
update msg model =
    case msg of
        Select index ->
            { model | slide = index }


type alias Stat =
    { label : String, value : String }


type alias CodeEvidence =
    { title : String, file : String, excerpt : String }


stats : List Stat
stats =
    ${elmList(statRows.map(([label, value]) => `{ label = ${elmString(label)}, value = ${elmString(value)} }`))}


files : List ( String, String )
files =
    ${elmList(fileRows.map(([titleValue, file]) => `( ${elmString(titleValue)}, ${elmString(file)} )`))}


evidence : List CodeEvidence
evidence =
    ${elmList(codeRows.map(([titleValue, file, excerpt]) => `{ title = ${elmString(titleValue)}, file = ${elmString(file)}, excerpt = ${elmString(excerpt)} }`))}


view : Model -> Html Msg
view model =
    div [ class "generated-reviewer" ]
        [ div [ class "generated-reviewer__nav" ]
            [ tab model 0 "Review"
            , tab model 1 "Why"
            , tab model 2 "Evidence"
            , tab model 3 "Flow"
            ]
        , div [ class "generated-reviewer__slide" ]
            [ case model.slide of
                0 ->
                    reviewSlide

                1 ->
                    whySlide

                2 ->
                    evidenceSlide

                _ ->
                    flowSlide
            ]
        ]


tab : Model -> Int -> String -> Html Msg
tab model index label =
    button
        [ class (if model.slide == index then "generated-reviewer__tab generated-reviewer__tab--active" else "generated-reviewer__tab")
        , onClick (Select index)
        ]
        [ text label ]


reviewSlide : Html Msg
reviewSlide =
    section []
        [ span [ class "generated-reviewer__kicker" ] [ text "Review" ]
        , h1 [] [ text ${elmString(title)} ]
        , p [] [ text ${elmString(lead)} ]
        , div [ class "generated-reviewer__stats" ] (List.map viewStat stats)
        ]


whySlide : Html Msg
whySlide =
    section []
        [ span [ class "generated-reviewer__kicker" ] [ text "Reviewer thesis" ]
        , h2 [] [ text "What changed and why" ]
        , ol []
            [ li [] [ text ${elmString(`${changed.length || 0} changed nodes are primary review surface.`)} ]
            , li [] [ text ${elmString(`${context?.changedFilesTotal ?? 0} files changed in selected target.`)} ]
            , li [] [ text ${elmString(`Start with ${firstFile}, then walk incoming and outgoing graph edges.`)} ]
            , li [] [ text ${elmString(`Review target: ${targetSummary}.`)} ]
            , li [] [ text ${elmString('Risk comes from graph-neighbor behavior, not from isolated diff chunks.')} ]
            ]
        ]


evidenceSlide : Html Msg
evidenceSlide =
    section []
        [ span [ class "generated-reviewer__kicker" ] [ text "Source evidence" ]
        , h2 [] [ text "Code excerpts" ]
        , div [] (List.map viewEvidence evidence)
        ]


flowSlide : Html Msg
flowSlide =
    section []
        [ span [ class "generated-reviewer__kicker" ] [ text "Graph explanation" ]
        , h2 [] [ text "Review flow" ]
        , viewGraph
        , div [ class "generated-reviewer__files" ] (List.map viewFile files)
        ]


viewStat : Stat -> Html Msg
viewStat stat =
    div [ class "generated-reviewer__stat" ]
        [ span [] [ text stat.label ]
        , strong [] [ text stat.value ]
        ]


viewEvidence : CodeEvidence -> Html Msg
viewEvidence item =
    div [ class "generated-reviewer__code-card" ]
        [ div [ class "generated-reviewer__code-head" ]
            [ strong [] [ text item.title ]
            , span [] [ text item.file ]
            ]
        , pre [] [ code [] [ text item.excerpt ] ]
        ]


viewFile : ( String, String ) -> Html Msg
viewFile ( label, file ) =
    div [ class "generated-reviewer__file" ]
        [ strong [] [ text label ]
        , span [] [ text file ]
        ]


viewGraph : Html Msg
viewGraph =
    svg [ SA.viewBox "0 0 520 150", SA.class "generated-reviewer__svg" ]
        [ line [ SA.x1 "72", SA.y1 "75", SA.x2 "260", SA.y2 "75", SA.stroke "#72d0b6", SA.strokeWidth "3" ] []
        , line [ SA.x1 "260", SA.y1 "75", SA.x2 "448", SA.y2 "75", SA.stroke "#d8b44a", SA.strokeWidth "3", SA.strokeDasharray "10 8" ] []
        , circle [ SA.cx "72", SA.cy "75", SA.r "34", SA.fill "#1f3b46", SA.stroke "#72d0b6", SA.strokeWidth "3" ] []
        , circle [ SA.cx "260", SA.cy "75", SA.r "38", SA.fill "#3a2630", SA.stroke "#d8b44a", SA.strokeWidth "3" ] []
        , circle [ SA.cx "448", SA.cy "75", SA.r "34", SA.fill "#242a42", SA.stroke "#9da7ff", SA.strokeWidth "3" ] []
        ]
`;
  }

  function contextToSnippets(context) {
    const changedFiles = Array.isArray(context?.changedFiles) ? context.changedFiles : [];
    return changedFiles.slice(0, 3).map((file) => {
      const excerptLines = Array.isArray(file?.diffExcerpt) ? file.diffExcerpt : [];
      const excerpt = excerptLines.slice(0, 14).join('\n').trim();
      return {
        title: String(file?.path || '(unknown file)'),
        file: String(file?.path || '(unknown file)'),
        excerpt: excerpt || `path ${String(file?.path || '(unknown file)')}\n-- diff excerpt unavailable`,
      };
    });
  }

  function normalizeGeneratedReviewerResult(result) {
    if (typeof result === 'string') {
      return { bundleJs: result, source: '' };
    }
    return {
      bundleJs: String(result?.bundleJs || result?.bundle_js || '').trim(),
      source: String(result?.source || result?.elmSource || result?.elm_source || '').trim(),
    };
  }

  function formatBackendRejection(targetSummary, message) {
    const detail = String(message || 'unknown backend error').trim() || 'unknown backend error';
    return `Reviewer backend rejected target "${targetSummary}": ${detail}`;
  }

  async function pauseForPendingState() {
    await new Promise((resolve) => setTimeout(resolve, 300));
  }

  async function compile(kind) {
    const prompt = String(question || '').trim();
    if (kind === 'question' && !prompt) {
      error = 'Question required.';
      status = 'error';
      return;
    }

    error = '';
    status = 'compiling';
    bundleJs = '';

    try {
      const reviewNodes = sortReviewNodes(nodes).slice(0, 6);
      const normalizedTarget = normalizeReviewTarget(reviewTarget);
      const targetSummary = formatReviewTargetSummary(normalizedTarget);
      const context = getReviewerTargetContext
        ? await getReviewerTargetContext(normalizedTarget)
        : null;
      const contextSnippets = contextToSnippets(context);
      const snippets = contextSnippets.length > 0
        ? contextSnippets
        : await loadEvidence(reviewNodes);
      const graphSummary = {
        visibleNodeCount: nodes.length,
        visibleEdgeCount: edges.length,
        canonicalEntityCount: graphMeta?.canonicalEntityCount || nodes.length,
        canonicalEdgeCount: graphMeta?.canonicalEdgeCount || edges.length,
      };

      let backendRejection = null;
      if (generateReviewerElm) {
        try {
          const generated = await generateReviewerElm({
            kind,
            question: prompt,
            target: normalizedTarget,
            graphMeta: graphSummary,
            context,
            snippets,
            reviewNodes,
          });
          const generatedResult = normalizeGeneratedReviewerResult(generated);
          if (generatedResult.bundleJs) {
            await pauseForPendingState();
            bundleJs = generatedResult.bundleJs;
            status = 'mounted';
            return;
          }
          if (generatedResult.source) {
            if (!compileReviewerElm) {
              throw new Error('Elm compiler bridge missing for backend source result.');
            }
            bundleJs = await compileReviewerElm(generatedResult.source);
            await pauseForPendingState();
            status = 'mounted';
            return;
          }
          backendRejection = formatBackendRejection(targetSummary, 'empty response payload');
        } catch (backendError) {
          backendRejection = formatBackendRejection(targetSummary, backendError?.message || String(backendError));
        }
      }

      if (!compileReviewerElm) {
        throw new Error(backendRejection || 'Elm compiler bridge missing.');
      }
      const source = generatedElmSource(kind, prompt, reviewNodes, snippets, normalizedTarget, context);
      await pauseForPendingState();
      bundleJs = await compileReviewerElm(source);
      status = 'mounted';
    } catch (err) {
      error = err?.message || String(err);
      status = 'error';
    }
  }
</script>

<Window
  title="Code reviewer"
  className="eyeloss-reviewer-window"
  style="z-index: 1407;"
  {visible}
  {x}
  {y}
  width={windowWidth}
  height={windowHeight}
  minWidth={520}
  minHeight={500}
  {containerWidth}
  {containerHeight}
  onfocus={onfocus}
  onclose={onclose}
  ondragEnd={ondragEnd}
  onresizeEnd={onresizeEnd}
>
  <div class="eyeloss-reviewer" onwheel={(event) => event.stopPropagation()}>
    <header class="eyeloss-reviewer__header">
      <div>
        <div class="eyeloss-reviewer__eyebrow">Review workspace</div>
        <h2>Code reviewer</h2>
      </div>
      <div class={`eyeloss-reviewer__status eyeloss-reviewer__status--${status}`}>{status}</div>
    </header>

    <section class="eyeloss-reviewer__target">
      <div class="eyeloss-reviewer__target-grid">
        <label>
          <span>Target mode</span>
          <select
            class="input-mono"
            value={activeReviewTarget.mode}
            onchange={(event) => updateReviewTarget({ mode: event.currentTarget.value })}
          >
            <option value="working_copy">Working copy</option>
            <option value="since">Since revset</option>
            <option value="range">Base..head range</option>
          </select>
        </label>
        <label>
          <span>Label</span>
          <input
            type="text"
            class="input-mono"
            value={activeReviewTarget.label}
            placeholder="Displayed target label."
            oninput={(event) => updateReviewTarget({ label: event.currentTarget.value })}
          />
        </label>
        {#if activeReviewTarget.mode === 'since'}
          <label>
            <span>Since</span>
            <input
              type="text"
              class="input-mono"
              value={activeReviewTarget.since}
              placeholder="@"
              oninput={(event) => updateReviewTarget({ since: event.currentTarget.value })}
            />
          </label>
        {/if}
        {#if activeReviewTarget.mode === 'range'}
          <label>
            <span>Base</span>
            <input
              type="text"
              class="input-mono"
              value={activeReviewTarget.base}
              placeholder="main~1"
              oninput={(event) => updateReviewTarget({ base: event.currentTarget.value })}
            />
          </label>
          <label>
            <span>Head</span>
            <input
              type="text"
              class="input-mono"
              value={activeReviewTarget.head}
              placeholder="HEAD"
              oninput={(event) => updateReviewTarget({ head: event.currentTarget.value })}
            />
          </label>
        {/if}
        <label class="eyeloss-reviewer__toggle">
          <input
            type="checkbox"
            checked={activeReviewTarget.includeWorkingCopy}
            onchange={(event) => updateReviewTarget({ includeWorkingCopy: event.currentTarget.checked })}
          />
          <span>Include working copy</span>
        </label>
        <div class="eyeloss-reviewer__target-summary">{formatReviewTargetSummary(activeReviewTarget)}</div>
      </div>
    </section>

    <section class="eyeloss-reviewer__composer">
      <label>
        <span>Question</span>
        <textarea
          aria-label="Review question"
          class="input-mono"
          rows="2"
          bind:value={question}
          placeholder="Ask follow-up after branch review."
        ></textarea>
      </label>
      <div class="eyeloss-reviewer__actions">
        <button type="button" class="btn" onclick={() => compile('branch')} disabled={status === 'compiling'}>Review branch</button>
        <button type="button" class="btn btn-primary" onclick={() => compile('question')} disabled={status === 'compiling'}>Ask</button>
      </div>
    </section>

    {#if error}
      <div class="eyeloss-reviewer__error">{error}</div>
    {/if}

    {#if status === 'compiling'}
      <div class="eyeloss-reviewer__pending">Preparing review.</div>
    {/if}

    <GeneratedElmSurface
      surfaceId="reviewer"
      {bundleJs}
      status="Review waiting."
      onerror={(message) => {
        error = message;
        status = 'error';
      }}
    />
  </div>
</Window>

<style>
  .eyeloss-reviewer-window :global(.window-content) {
    padding: 0;
    overflow: hidden;
  }

  .eyeloss-reviewer {
    display: grid;
    grid-template-rows: auto auto auto auto auto minmax(0, 1fr);
    height: 100%;
    background: var(--bg-100);
    color: var(--text);
  }

  .eyeloss-reviewer__header {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    align-items: flex-start;
    padding: 16px;
    border-bottom: 1px solid var(--bg-300);
    background:
      linear-gradient(135deg, color-mix(in srgb, var(--primary) 16%, transparent), color-mix(in srgb, var(--secondary) 10%, transparent)),
      var(--bg-100);
  }

  .eyeloss-reviewer__header h2 {
    margin: 2px 0 0;
    font-size: 22px;
  }

  .eyeloss-reviewer__eyebrow {
    font-size: 11px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-dim);
  }

  .eyeloss-reviewer__status {
    padding: 6px 10px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    color: var(--text);
    font-size: 11px;
    text-transform: uppercase;
  }

  .eyeloss-reviewer__composer {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 10px;
    align-items: end;
    padding: 14px 16px;
    border-bottom: 1px solid var(--bg-300);
  }

  .eyeloss-reviewer__target {
    padding: 12px 16px;
    border-bottom: 1px solid var(--bg-300);
    background: var(--bg-200);
  }

  .eyeloss-reviewer__target-grid {
    display: grid;
    gap: 10px;
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .eyeloss-reviewer__target label {
    display: grid;
    gap: 6px;
    font-size: 12px;
    min-width: 0;
  }

  .eyeloss-reviewer__target input,
  .eyeloss-reviewer__target select {
    width: 100%;
    min-width: 0;
  }

  .eyeloss-reviewer__toggle {
    grid-column: 1 / -1;
    display: flex !important;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }

  .eyeloss-reviewer__toggle input {
    width: auto;
  }

  .eyeloss-reviewer__target-summary {
    grid-column: 1 / -1;
    font-size: 12px;
    color: var(--text-dim);
    border: 1px solid var(--bg-300);
    background: var(--bg-100);
    padding: 8px 10px;
  }

  .eyeloss-reviewer__composer label {
    display: grid;
    gap: 6px;
    font-size: 12px;
  }

  .eyeloss-reviewer__composer textarea {
    resize: none;
    width: 100%;
  }

  .eyeloss-reviewer__actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .eyeloss-reviewer__error,
  .eyeloss-reviewer__pending {
    margin: 10px 16px 0;
    padding: 10px 12px;
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
    font-size: 12px;
  }

  .eyeloss-reviewer__error {
    border-color: color-mix(in srgb, var(--red) 52%, var(--bg-300));
    background: color-mix(in srgb, var(--red) 12%, var(--bg-100));
    color: var(--red);
  }

  .eyeloss-reviewer :global(.generated-reviewer) {
    min-height: 0;
    height: 100%;
    display: grid;
    grid-template-columns: 128px minmax(0, 1fr);
    color: var(--text);
    background: var(--bg-100);
  }

  .eyeloss-reviewer :global(.generated-reviewer__nav) {
    padding: 12px;
    display: grid;
    align-content: start;
    gap: 8px;
    border-right: 1px solid var(--bg-300);
    background: var(--bg-200);
  }

  .eyeloss-reviewer :global(.generated-reviewer__tab) {
    padding: 10px;
    border: 1px solid var(--bg-300);
    background: var(--bg-100);
    color: inherit;
    text-align: left;
    cursor: pointer;
  }

  .eyeloss-reviewer :global(.generated-reviewer__tab--active) {
    border-color: var(--secondary);
    background: color-mix(in srgb, var(--secondary) 14%, var(--bg-100));
  }

  .eyeloss-reviewer :global(.generated-reviewer__slide) {
    min-width: 0;
    min-height: 0;
    overflow: auto;
    padding: 22px;
  }

  .eyeloss-reviewer :global(.generated-reviewer__kicker) {
    display: block;
    margin-bottom: 8px;
    font-size: 11px;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--primary);
  }

  .eyeloss-reviewer :global(.generated-reviewer h1),
  .eyeloss-reviewer :global(.generated-reviewer h2) {
    margin: 0 0 12px;
    font-size: 28px;
    line-height: 1.08;
  }

  .eyeloss-reviewer :global(.generated-reviewer p),
  .eyeloss-reviewer :global(.generated-reviewer li) {
    font-size: 14px;
    line-height: 1.45;
  }

  .eyeloss-reviewer :global(.generated-reviewer__stats) {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
  }

  .eyeloss-reviewer :global(.generated-reviewer__stat),
  .eyeloss-reviewer :global(.generated-reviewer__code-card),
  .eyeloss-reviewer :global(.generated-reviewer__file) {
    border: 1px solid var(--bg-300);
    background: var(--bg-200);
  }

  .eyeloss-reviewer :global(.generated-reviewer__stat) {
    padding: 12px;
  }

  .eyeloss-reviewer :global(.generated-reviewer__stat span) {
    display: block;
    margin-bottom: 8px;
    font-size: 11px;
    color: var(--text-dim);
  }

  .eyeloss-reviewer :global(.generated-reviewer__stat strong) {
    font-size: 22px;
  }

  .eyeloss-reviewer :global(.generated-reviewer__code-card) {
    margin-bottom: 12px;
  }

  .eyeloss-reviewer :global(.generated-reviewer__code-head) {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--bg-300);
    font-size: 12px;
  }

  .eyeloss-reviewer :global(.generated-reviewer pre) {
    margin: 0;
    padding: 12px;
    max-height: 220px;
    overflow: auto;
    background: var(--bg-100);
    font-size: 12px;
    line-height: 1.45;
  }

  .eyeloss-reviewer :global(.generated-reviewer__svg) {
    width: 100%;
    height: auto;
    margin: 8px 0 16px;
  }

  .eyeloss-reviewer :global(.generated-reviewer__files) {
    display: grid;
    gap: 8px;
  }

  .eyeloss-reviewer :global(.generated-reviewer__file) {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    font-size: 12px;
  }
</style>
