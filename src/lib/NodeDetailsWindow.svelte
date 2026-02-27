<script>
  import Window from './Window.svelte';
  import Editor from './Editor.svelte';

  let {
    node,
    width = 0,
    height = 0,
    x = 24,
    y = 24,
    windowWidth = 640,
    windowHeight = 520,
    active = false,
    theme = 'midnight',
    getNodeHeat = () => 0,
    getFileDiff,
    getFileSource,
    saveFile,
    diffCache,
    sourceCache,
    sourceReferences = [],
    onOpenReference = null,
    onclose = null,
    onfocus = null,
    ondragEnd = null,
    onresizeEnd = null,
  } = $props();

  let fileDiff = $state(null);
  let fileSource = $state(null);
  let viewMode = $state('diff'); // 'diff' | 'source'
  let diffLoading = $state(false);
  let sourceLoading = $state(false);
  let diffRequestFile = $state(null);
  let sourceRequestFile = $state(null);
  let isSaving = $state(false);
  let showSignatures = $state(false);
  let expandedChunks = $state(new Set());

  let diffLines = $derived.by(() => {
    if (!fileDiff) return [];
    return fileDiff.split('\n').filter((line) =>
      !line.startsWith('diff --git') &&
      !line.startsWith('index ') &&
      !line.startsWith('--- ') &&
      !line.startsWith('+++ ')
    );
  });

  let splitDiffLines = $derived.by(() => {
    if (!fileDiff) return [];

    let leftNum = 0;
    let rightNum = 0;
    let pairs = [];

    for (let i = 0; i < diffLines.length; i++) {
      const line = diffLines[i];
      if (line.startsWith('@@')) {
        const match = line.match(/@@ -(\d+),?\d* \+(\d+),?\d* @@/);
        if (match) {
          leftNum = parseInt(match[1], 10);
          rightNum = parseInt(match[2], 10);
        }
        pairs.push({ type: 'meta', leftNum: '', rightNum: '', text: line });
      } else if (line.startsWith('-')) {
        pairs.push({ type: 'del', leftNum: leftNum++, rightNum: '', text: line.substring(1) });
      } else if (line.startsWith('+')) {
        pairs.push({ type: 'add', leftNum: '', rightNum: rightNum++, text: line.substring(1) });
      } else {
        pairs.push({ type: 'context', leftNum: leftNum++, rightNum: rightNum++, text: line.substring(1) });
      }
    }

    let aligned = [];
    for (let i = 0; i < pairs.length; i++) {
      if (pairs[i].type === 'del') {
        let delLines = [pairs[i]];
        let j = i + 1;
        while (j < pairs.length && pairs[j].type === 'del') {
          delLines.push(pairs[j]);
          j++;
        }

        let addLines = [];
        while (j < pairs.length && pairs[j].type === 'add') {
          addLines.push(pairs[j]);
          j++;
        }

        const maxLen = Math.max(delLines.length, addLines.length);
        for (let k = 0; k < maxLen; k++) {
          aligned.push({
            left: delLines[k] || null,
            right: addLines[k] || null,
          });
        }
        i = j - 1;
      } else if (pairs[i].type === 'add') {
        let addLines = [pairs[i]];
        let j = i + 1;
        while (j < pairs.length && pairs[j].type === 'add') {
          addLines.push(pairs[j]);
          j++;
        }
        for (let k = 0; k < addLines.length; k++) {
          aligned.push({ left: null, right: addLines[k] });
        }
        i = j - 1;
      } else {
        aligned.push({ left: pairs[i], right: pairs[i] });
      }
    }

    return aligned;
  });

  let diffChunks = $derived.by(() => {
    let chunks = [];
    let currentChunk = null;

    for (const line of splitDiffLines) {
      const metaText =
        line.left?.type === 'meta'
          ? line.left.text
          : line.right?.type === 'meta'
            ? line.right.text
            : null;

      if (metaText) {
        if (currentChunk) chunks.push(currentChunk);

        const contextMatch = metaText.match(/@@[^{]*@@(.*)/);
        let context = contextMatch && contextMatch[1] ? contextMatch[1].trim() : '';
        if (!context) context = metaText.trim();

        currentChunk = { context, lines: [] };
      } else {
        if (!currentChunk) currentChunk = { context: 'Header', lines: [] };
        currentChunk.lines.push(line);
      }
    }

    if (currentChunk) chunks.push(currentChunk);
    return chunks;
  });

  $effect(() => {
    if (!fileDiff) {
      expandedChunks = new Set();
      return;
    }
    const next = new Set();
    diffChunks.forEach((_, i) => next.add(i));
    expandedChunks = next;
  });

  function toggleChunk(index) {
    const next = new Set(expandedChunks);
    if (next.has(index)) next.delete(index);
    else next.add(index);
    expandedChunks = next;
  }

  function getShortName(id) {
    if (!id) return '';

    if (id.includes('/')) {
      const parts = id.split('/');
      return parts[parts.length - 1];
    }

    if (id.includes('.')) {
      const parts = id.split('.');
      const last = parts[parts.length - 1];

      const knownExts = [
        'ex', 'exs', 'svelte', 'js', 'ts', 'jsx', 'tsx', 'rs', 'py', 'rb', 'go', 'java', 'cpp', 'h', 'php', 'cs', 'json', 'md',
      ];
      if (knownExts.includes(last.toLowerCase())) return id;
      return last;
    }

    return id;
  }

  function findReferenceByToken(token) {
    if (!token) return null;
    const exact = sourceReferences.find((ref) => ref.token === token);
    if (exact) return exact;
    return sourceReferences.find((ref) => ref.token?.toLowerCase() === token.toLowerCase()) || null;
  }

  function handleEditorTokenClick(token) {
    const ref = findReferenceByToken(token);
    if (!ref || !onOpenReference) return;
    onOpenReference(ref);
  }

  async function handleSave() {
    if (!node?.file || fileSource === null || !saveFile) return;
    isSaving = true;
    try {
      await saveFile(node.file, fileSource);
      sourceCache?.set(node.file, fileSource || '');
    } catch (e) {
      console.error('Failed to save file', e);
    } finally {
      isSaving = false;
    }
  }

  $effect(() => {
    const file = node?.file;
    if (!file) {
      fileDiff = null;
      fileSource = null;
      diffLoading = false;
      sourceLoading = false;
      diffRequestFile = null;
      sourceRequestFile = null;
      return;
    }

    fileDiff = diffCache?.has(file) ? diffCache.get(file) : null;
    fileSource = sourceCache?.has(file) ? sourceCache.get(file) : null;

    if (!diffCache?.has(file) && diffRequestFile !== file && getFileDiff) {
      diffLoading = true;
      diffRequestFile = file;
      getFileDiff(file)
        .then((diff) => {
          diffCache?.set(file, diff);
          if (node?.file === file) fileDiff = diff;
        })
        .finally(() => {
          if (diffRequestFile === file) diffRequestFile = null;
          if (node?.file === file) diffLoading = false;
        });
    } else if (diffRequestFile !== file) {
      diffLoading = false;
    }

    if (!sourceCache?.has(file) && sourceRequestFile !== file && getFileSource) {
      sourceLoading = true;
      sourceRequestFile = file;
      getFileSource(file)
        .then((source) => {
          sourceCache?.set(file, source);
          if (node?.file === file) fileSource = source;
        })
        .finally(() => {
          if (sourceRequestFile === file) sourceRequestFile = null;
          if (node?.file === file) sourceLoading = false;
        });
    } else if (sourceRequestFile !== file) {
      sourceLoading = false;
    }
  });
</script>

{#if node}
  <Window
    title={getShortName(node.id)}
    width={windowWidth}
    height={windowHeight}
    {x}
    {y}
    containerWidth={width}
    containerHeight={height}
    {active}
    onclose={onclose}
    onfocus={onfocus}
    ondragEnd={ondragEnd}
    onresizeEnd={onresizeEnd}
  >
    <div class="eyeloss-node-details__body eyeloss-node-details__body--window" onwheel={(e) => e.stopPropagation()}>
      <div class="eyeloss-node-details__meta" style="margin-bottom: 8px; font-size: 11px; display: flex; justify-content: space-between; align-items: center;">
        <span>{node.file || 'unknown'}  |  {node.line_count || '--'} lines</span>
        <div class="tabs" style="display: flex; gap: 4px;">
          <button class="btn {viewMode === 'diff' ? 'btn-primary' : ''}" style="font-size: 0.6rem; padding: 2px 6px;" onclick={() => (viewMode = 'diff')}>Diff</button>
          <button class="btn {viewMode === 'source' ? 'btn-primary' : ''}" style="font-size: 0.6rem; padding: 2px 6px;" onclick={() => (viewMode = 'source')}>Source</button>
        </div>
      </div>

      {#if node.functions && node.functions.length > 0}
        <button
          class="eyeloss-node-details__section-title eyeloss-node-details__section-title--toggle"
          type="button"
          onclick={(e) => {
            e.stopPropagation();
            showSignatures = !showSignatures;
          }}
          style="background: rgba(255,255,255,0.05);"
        >
          <span>{showSignatures ? 'v' : '>'} Signatures ({node.functions.length})</span>
        </button>
        {#if showSignatures}
          <ul class="eyeloss-node-details__sigs" style="padding: 8px;">
            {#each node.functions as fn}
              <li class="eyeloss-node-details__sig">
                <span class="eyeloss-node-details__sig-type">{fn.kind}</span>
                <span class="eyeloss-node-details__sig-name">{fn.name}/{fn.arity}</span>
              </li>
            {/each}
          </ul>
        {/if}
      {/if}

      {#if viewMode === 'diff'}
        {#if diffLoading}
          <div class="eyeloss-node-details__meta" style="margin-top: 16px;">Loading diff...</div>
        {:else if fileDiff}
          <div class="eyeloss-node-details__section-title" style="margin-top: 16px; padding-bottom: 4px; border-bottom: 1px solid rgba(255,255,255,0.1);">
            <span>Diff Changes</span>
          </div>

          <div class="eyeloss-split-diff" style:filter={`brightness(${1 + getNodeHeat(node.file) * 0.5})`}>
            {#each diffChunks as chunk, i}
              <div class="diff-chunk-wrapper">
                <button class="diff-chunk-header" type="button" onclick={() => toggleChunk(i)}>
                  <span class="diff-chunk-icon">{expandedChunks.has(i) ? 'v' : '>'}</span>
                  <span class="diff-chunk-context">{chunk.context}</span>
                </button>

                {#if expandedChunks.has(i)}
                  <div class="diff-chunk-content">
                    <div class="split-diff-half">
                      {#each chunk.lines as line}
                        <div class="split-diff-line {line.left ? 'type-' + line.left.type : 'type-empty'}">
                          <span class="split-diff-num">{line.left ? line.left.leftNum : ''}</span>
                          <span class="split-diff-text">{line.left ? line.left.text : ' '}</span>
                        </div>
                      {/each}
                    </div>
                    <div class="split-diff-half">
                      {#each chunk.lines as line}
                        <div class="split-diff-line {line.right ? 'type-' + line.right.type : 'type-empty'}">
                          <span class="split-diff-num">{line.right ? line.right.rightNum : ''}</span>
                          <span class="split-diff-text">{line.right ? line.right.text : ' '}</span>
                        </div>
                      {/each}
                    </div>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      {:else}
        {#if sourceLoading}
          <div class="eyeloss-node-details__meta" style="margin-top: 16px;">Loading source...</div>
        {:else if fileSource !== null}
          <div class="eyeloss-node-details__section-title" style="margin-top: 16px; padding-bottom: 4px; display: flex; justify-content: space-between; align-items: center;">
            <span>Full Source Code</span>
            <button
              class="btn {isSaving ? 'btn-ghost' : 'btn-primary'}"
              style="font-size: 0.6rem; padding: 2px 8px;"
              onclick={handleSave}
              disabled={isSaving}
            >
              {isSaving ? 'Saving...' : 'Save Changes'}
            </button>
          </div>
          {#if sourceReferences.length > 0}
            <div class="eyeloss-source-links">
              <span class="eyeloss-source-links__label">References</span>
              {#each sourceReferences as ref}
                <button
                  type="button"
                  class="eyeloss-source-links__item"
                  title={`Open source: ${ref.file}`}
                  onclick={() => onOpenReference && onOpenReference(ref)}
                >
                  {ref.token}
                </button>
              {/each}
            </div>
          {/if}
          <Editor
            bind:value={fileSource}
            file={node.file}
            {theme}
            clickableTokens={sourceReferences.map((ref) => ref.token)}
            onTokenClick={handleEditorTokenClick}
          />
        {/if}
      {/if}
    </div>
  </Window>
{/if}
