import { listen } from '@tauri-apps/api/event';

const stage = document.getElementById('stage');
const sprite = document.getElementById('sprite');

listen('mabel-companion-state', (e) => {
  const { facing, sitting, blink } = e.payload || {};
  console.log('[companion] state event', e.payload);

  stage.classList.toggle('facing-left', facing === 'left');
  sprite.classList.toggle('sitting', !!sitting && !blink);

  if (blink) {
    sprite.classList.add('blinking');
    sprite.classList.remove('sitting');
    setTimeout(() => {
      sprite.classList.remove('blinking');
      if (sitting) sprite.classList.add('sitting');
    }, 180);
  } else if (!sitting) {
    sprite.classList.remove('blinking');
  }
}).then(() => console.log('[companion] listening for state events'));

listen('mabel-companion-skin', (e) => {
  const skin = (e.payload && e.payload.skin) || 'mabel';
  document.body.classList.toggle('mochi-mode', skin === 'mochi');
  console.log('[companion] skin event', skin);
}).then(() => console.log('[companion] listening for skin events'));
