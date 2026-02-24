<script>
  import { onDestroy } from 'svelte';
  import { clamp } from './coord_utils.js';

  let {
    x = $bindable(0),
    y = $bindable(0),
    width = $bindable(300),
    height = $bindable(200),
    minWidth = 240,
    minHeight = 200,
    aspectRatio = null,
    extraHeight = 0,
    containerWidth = 0,
    containerHeight = 0,
    className = "",
    style = "",
    active = false,
    visible = true,
    title = "",
    breadcrumbStack = [],
    bodyClass = "",
    onfocus = null,
    onclose = null,
    onresizeEnd = null,
    ondragEnd = null,
    onheaderDblClick = null,
    onbreadcrumbclick = null,
    children,
    header
  } = $props();

  let dragging = $state(false);
  let resizing = $state(false);
  let dragStartOffset = $state({ x: 0, y: 0 });
  let resizeStartDim = $state({ width: 0, height: 0, x: 0, y: 0 });

  let _x = $state(x);
  let _y = $state(y);
  let _width = $state(width);
  let _height = $state(height);

  $effect(() => {
    if (!dragging && !resizing) {
      _x = x;
      _y = y;
      _width = width;
      _height = height;
    }
  });

  const finalStyle = $derived(`left: ${_x}px; top: ${_y}px; width: ${_width}px; height: ${_height}px; ${style}`);

  function handleActivate() {
    if (onfocus) onfocus();
  }

  function handleDragStart(event) {
    if (event.target.closest('button')) return;
    event.stopPropagation();
    handleActivate();

    dragging = true;
    dragStartOffset = {
      x: event.clientX - _x,
      y: event.clientY - _y
    };

    window.addEventListener('mousemove', onGlobalMove);
    window.addEventListener('mouseup', endInteraction);
  }

  function handleResizeStart(event) {
    event.preventDefault();
    event.stopPropagation();
    handleActivate();

    resizing = true;
    resizeStartDim = {
      x: event.clientX,
      y: event.clientY,
      width: _width,
      height: _height
    };

    window.addEventListener('mousemove', onGlobalMove);
    window.addEventListener('mouseup', endInteraction);
  }

  function onGlobalMove(event) {
    if (dragging) {
      updatePosition(event);
    } else if (resizing) {
      updateSize(event);
    }
  }

  function updatePosition(event) {
    let nextX = event.clientX - dragStartOffset.x;
    let nextY = event.clientY - dragStartOffset.y;

    const visibleThreshold = 40;
    if (containerWidth && containerHeight) {
      const minX = visibleThreshold - _width;
      const maxX = containerWidth - visibleThreshold;
      const minY = 0;
      const maxY = containerHeight - visibleThreshold;

      _x = clamp(nextX, minX, maxX);
      _y = clamp(nextY, minY, maxY);
    } else {
      _x = nextX;
      _y = nextY;
    }
  }

  function updateSize(event) {
    const dx = event.clientX - resizeStartDim.x;
    const dy = event.clientY - resizeStartDim.y;

    let newW = Math.max(minWidth, resizeStartDim.width + dx);
    let newH;

    if (aspectRatio) {
      newH = (newW / aspectRatio) + extraHeight;
    } else {
      newH = Math.max(minHeight, resizeStartDim.height + dy);
    }

    if (containerWidth && containerHeight) {
      if (newW > containerWidth) {
        newW = containerWidth;
        if (aspectRatio) newH = (newW / aspectRatio) + extraHeight;
      }
      if (newH > containerHeight) {
        newH = containerHeight;
        if (aspectRatio) {
          newW = (newH - extraHeight) * aspectRatio;
        }
      }
    }

    _width = newW;
    _height = newH;
  }

  function endInteraction() {
    if (dragging) {
      dragging = false;
      x = _x;
      y = _y;
      if (ondragEnd) ondragEnd({ detail: { x: _x, y: _y } });
    }
    if (resizing) {
      resizing = false;
      width = _width;
      height = _height;
      if (onresizeEnd) onresizeEnd({ detail: { width: _width, height: _height } });
    }

    window.removeEventListener('mousemove', onGlobalMove);
    window.removeEventListener('mouseup', endInteraction);
  }

  onDestroy(() => {
    if (typeof window !== 'undefined') {
      window.removeEventListener('mousemove', onGlobalMove);
      window.removeEventListener('mouseup', endInteraction);
    }
  });
</script>

{#if visible}
  <div
    class={`window ${className} ${active ? 'window--active' : ''}`}
    style={finalStyle}
    role="dialog"
    tabindex="-1"
    onpointerdown={(e) => { e.stopPropagation(); handleActivate(); }}
    onmousedown={(e) => { e.stopPropagation(); handleActivate(); }}
    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') handleActivate(); }}
    onclick={(e) => e.stopPropagation()}
  >
    {#if dragging || resizing}
      <div class="window-glass-pane"></div>
    {/if}

    {#if header}
      {@render header({ startDrag: handleDragStart })}
    {:else}
      <div
        class="window-header"
        role="presentation"
        onmousedown={handleDragStart}
        ondblclick={() => { if (onheaderDblClick) onheaderDblClick(); }}
      >
        <div class="window-header__title-container">
          {#if breadcrumbStack && breadcrumbStack.length > 1}
            <div class="breadcrumbs">
              {#each breadcrumbStack as crumb, i}
                {#if i < breadcrumbStack.length - 1}
                  <button 
                    class="breadcrumb-link" 
                    type="button" 
                    onclick={(e) => { e.stopPropagation(); if (onbreadcrumbclick) onbreadcrumbclick({ index: i, path: crumb.path }); }}
                  >
                    {crumb.title}
                  </button>
                  <span class="breadcrumb-separator">/</span>
                {:else}
                  <h3 class="window-title">{crumb.title}</h3>
                {/if}
              {/each}
            </div>
          {:else}
            <h3 class="window-title">{title}</h3>
          {/if}
        </div>
        <button class="window-close" type="button" onclick={() => { if (onclose) onclose(); }}>X</button>
      </div>
    {/if}

    <div class={`window-content ${bodyClass}`}>
      {#if children}
        {@render children()}
      {/if}
    </div>

    <div
      class="window-resize-handle"
      role="presentation"
      onmousedown={handleResizeStart}
    ></div>
  </div>
{/if}
