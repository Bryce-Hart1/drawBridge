// Claude — Date 11/03/2026
import { isTauri } from '@tauri-apps/api/core';
import { mount } from 'svelte';
import App from './App.svelte';
import './styles/tui.css';

// On macOS the window has no title bar of its own (tauri.conf.json,
// `titleBarStyle: Overlay`): the traffic lights sit in the top frame, and the
// title moves right to clear them.
if (isTauri() && navigator.userAgent.includes('Mac')) {
  document.documentElement.classList.add('mac-overlay');
}

mount(App, { target: document.getElementById('app')! });
