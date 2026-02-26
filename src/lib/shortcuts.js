export function setupShortcuts(onProjectChange, onSearch, onTab) {
  const handler = (event) => {
    const isMac = navigator.platform.toUpperCase().indexOf('MAC') >= 0;
    const cmdOrCtrl = isMac ? event.metaKey : event.ctrlKey;

    const target = event.target;
    const isInput =
      target instanceof HTMLElement &&
      (target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.isContentEditable);

    if (cmdOrCtrl && event.key === 'o') {
      event.preventDefault();
      onProjectChange?.();
      return;
    }

    if (!isInput) {
      if (event.key === '/') {
        event.preventDefault();
        onSearch?.();
        return;
      }

      if (event.key === 'Tab') {
        event.preventDefault();
        onTab?.(event.shiftKey ? -1 : 1);
        return;
      }
    }
  };

  window.addEventListener('keydown', handler);
  return () => window.removeEventListener('keydown', handler);
}
