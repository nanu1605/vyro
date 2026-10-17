// The phone app's page (1.4): its own entry and build (vite.phone.config.ts), served by the
// relay, so it loads none of the desktop app.
import { mount } from 'svelte';
import '../styles/theme.css';
import '../remote/page.css';
import { applyLook, savedLook } from '../themes';
import Phone from './Phone.svelte';

const target = document.getElementById('app');
if (!target) throw new Error('#app mount point missing');
// The look of the PC last seen until it tells it again.
applyLook(savedLook());

export default mount(Phone, { target });
