// The remote companion's page (P26): its own entry, so the phone loads neither the app's views
// nor its Tauri bridge.
import { mount } from 'svelte';
import '../styles/theme.css';
import './page.css';
import { applyLook, savedLook } from '../themes';
import Remote from './Remote.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('#app mount point missing');
// The owner's look from the last visit until the PC tells it again.
applyLook(savedLook());

export default mount(Remote, { target });
