import { mount } from 'svelte';
import './styles/theme.css';
import './styles/app.css';
import App from './App.svelte';
import { applyLook, savedLook } from './themes';
import Overlay from './Overlay.svelte';
import DesktopWidget from './DesktopWidget.svelte';
import { widgetIdFromHash } from './widgets';

const target = document.getElementById('app');
if (!target) throw new Error('#app mount point missing');

// The game overlay window loads the same page with #overlay (src-tauri/src/overlay.rs), a
// desktop widget's window with #widget=<id> (src-tauri/src/widgets.rs).
const overlay = location.hash === '#overlay';
const widget = !overlay && widgetIdFromHash(location.hash) != null;
if (overlay) document.documentElement.classList.add('overlay');
else if (widget) document.documentElement.classList.add('widget');
// The last look, before the first paint (the settings file corrects it once loaded).
if (!overlay) applyLook(savedLook());

export default mount(overlay ? Overlay : widget ? DesktopWidget : App, { target });
