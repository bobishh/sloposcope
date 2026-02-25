<script>
  import { onMount, onDestroy } from 'svelte';
  import { EditorView, basicSetup } from 'codemirror';
  import { languages } from '@codemirror/language-data';
  import { oneDark } from '@codemirror/theme-one-dark';
  import { EditorState, Compartment } from '@codemirror/state';
  import { LanguageDescription } from '@codemirror/language';

  let { value = $bindable(''), file = '', theme = 'midnight' } = $props();

  let editorContainer;
  let view;
  const languageConf = new Compartment();

  async function updateLanguage(filename) {
    if (!view) return;
    
    const desc = LanguageDescription.matchFilename(languages, filename) || 
                 LanguageDescription.matchLanguageName(languages, filename.split('.').pop() || '');

    if (desc) {
      const lang = await desc.load();
      view.dispatch({
        effects: languageConf.reconfigure(lang)
      });
    } else {
      view.dispatch({
        effects: languageConf.reconfigure([])
      });
    }
  }

  $effect(() => {
    // Handle external value changes (e.g. loading a new file)
    if (view && value !== view.state.doc.toString()) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: value }
      });
    }
  });

  $effect(() => {
    // Update language when file prop changes
    if (view && file) {
      updateLanguage(file);
    }
  });

  onMount(async () => {
    const state = EditorState.create({
      doc: value,
      extensions: [
        basicSetup,
        languageConf.of([]),
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

    if (file) updateLanguage(file);
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
