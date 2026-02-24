<script>
  import { onMount, onDestroy } from 'svelte';
  import { EditorView, basicSetup } from 'codemirror';
  import { javascript } from '@codemirror/lang-javascript';
  import { html } from '@codemirror/lang-html';
  import { markdown } from '@codemirror/lang-markdown';
  import { python } from '@codemirror/lang-python';
  import { rust } from '@codemirror/lang-rust';
  import { sql } from '@codemirror/lang-sql';
  import { oneDark } from '@codemirror/theme-one-dark';
  import { EditorState } from '@codemirror/state';

  let { value = $bindable(''), file = '', theme = 'midnight' } = $props();

  let editorContainer;
  let view;

  function getLanguage(filename) {
    if (filename.endsWith('.js') || filename.endsWith('.ts') || filename.endsWith('.svelte')) return javascript();
    if (filename.endsWith('.html') || filename.endsWith('.heex')) return html();
    if (filename.endsWith('.md')) return markdown();
    if (filename.endsWith('.py')) return python();
    if (filename.endsWith('.rs')) return rust();
    if (filename.endsWith('.sql')) return sql();
    // Ruby is often handled by StreamLanguage if @codemirror/lang-ruby is missing
    return [];
  }

  $effect(() => {
    // Handle external value changes (e.g. loading a new file)
    if (view && value !== view.state.doc.toString()) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: value }
      });
    }
  });

  onMount(() => {
    const state = EditorState.create({
      doc: value,
      extensions: [
        basicSetup,
        getLanguage(file),
        theme === 'midnight' ? oneDark : [],
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            value = update.state.doc.toString();
          }
        }),
        EditorView.theme({
          "&": { height: "500px" },
          ".cm-scroller": { overflow: "auto" }
        })
      ]
    });

    view = new EditorView({
      state,
      parent: editorContainer
    });
  });

  onDestroy(() => {
    if (view) view.destroy();
  });
</script>

<div class="code-editor-wrapper" bind:this={editorContainer}></div>

<style>
  .code-editor-wrapper {
    width: 100%;
    border: 1px solid var(--bg-300);
    background: var(--bg-100);
  }
</style>
