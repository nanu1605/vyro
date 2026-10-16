import { mount } from 'svelte';
import './styles/theme.css';
import './styles/app.css';
import App from './App.svelte';
import { applyLook, savedLook } from './themes';
import Overlay from './Overlay.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('#app mount point missing');

// The game overlay window loads the same page with #overlay (src-tauri/src/overlay.rs).
const overlay = location.hash === '#overlay';
if (overlay) document.documentElement.classList.add('overlay');
// The last look, before the first paint (the settings file corrects it once loaded).
else applyLook(savedLook());

export default mount(overlay ? Overlay : App, { target });
