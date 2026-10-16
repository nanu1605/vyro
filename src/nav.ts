// The app's views (P28): the sidebar's groups, their icons, and what the command palette can
// jump to. The ids are the old tab ids, so saved "last tab" choices and links keep working.

import type { Component } from 'svelte';
import Activity from '@lucide/svelte/icons/activity';
import AppWindow from '@lucide/svelte/icons/app-window';
import ChartLine from '@lucide/svelte/icons/chart-line';
import Cpu from '@lucide/svelte/icons/cpu';
import Fan from '@lucide/svelte/icons/fan';
import Gamepad2 from '@lucide/svelte/icons/gamepad-2';
import Gauge from '@lucide/svelte/icons/gauge';
import HardDrive from '@lucide/svelte/icons/hard-drive';
import HeartPulse from '@lucide/svelte/icons/heart-pulse';
import LayoutDashboard from '@lucide/svelte/icons/layout-dashboard';
import LayoutGrid from '@lucide/svelte/icons/layout-grid';
import Lightbulb from '@lucide/svelte/icons/lightbulb';
import Monitor from '@lucide/svelte/icons/monitor';
import Mouse from '@lucide/svelte/icons/mouse';
import PcCase from '@lucide/svelte/icons/pc-case';
import Settings from '@lucide/svelte/icons/settings';
import Thermometer from '@lucide/svelte/icons/thermometer';
import Wifi from '@lucide/svelte/icons/wifi';

export type Group = 'overview' | 'performance' | 'pc' | 'app';

export const GROUPS: Array<{ id: Group; label: string }> = [
  { id: 'overview', label: 'Overview' },
  { id: 'performance', label: 'Performance' },
  { id: 'pc', label: 'This PC' },
  { id: 'app', label: 'Vyro' },
];

export interface View {
  id: string;
  label: string;
  group: Group;
  icon: Component;
  /** What the view is for, for the page header and the command palette. */
  blurb: string;
}

export const VIEWS = [
  { id: 'dashboard', label: 'Dashboard', group: 'overview', icon: LayoutDashboard, blurb: 'Live readings at a glance' },
  { id: 'health', label: 'Health', group: 'overview', icon: HeartPulse, blurb: 'What needs a look, and diagnostics' },
  { id: 'insights', label: 'Insights', group: 'overview', icon: Lightbulb, blurb: 'Patterns and the Performance Score' },
  { id: 'gaming', label: 'Gaming', group: 'performance', icon: Gamepad2, blurb: 'Frame rates, sessions and the overlay' },
  { id: 'benchmark', label: 'Benchmark', group: 'performance', icon: Gauge, blurb: 'Measure this PC against itself' },
  { id: 'cooling', label: 'Cooling', group: 'performance', icon: Fan, blurb: 'Fans, pump and curves' },
  { id: 'history', label: 'History', group: 'performance', icon: ChartLine, blurb: 'Readings over time' },
  { id: 'mypc', label: 'My PC', group: 'pc', icon: PcCase, blurb: 'This PC in 3-D, with a thermal map' },
  { id: 'hardware', label: 'Hardware', group: 'pc', icon: Cpu, blurb: 'Parts, firmware and changes' },
  { id: 'storage', label: 'Storage', group: 'pc', icon: HardDrive, blurb: 'Drives, health and volumes' },
  { id: 'network', label: 'Network', group: 'pc', icon: Wifi, blurb: 'Adapters, traffic and ping' },
  { id: 'displays', label: 'Displays', group: 'pc', icon: Monitor, blurb: 'Monitors and what they support' },
  { id: 'devices', label: 'Devices', group: 'pc', icon: Mouse, blurb: 'Peripherals and batteries' },
  { id: 'system', label: 'System', group: 'pc', icon: AppWindow, blurb: 'Windows, security and start-up apps' },
  { id: 'processes', label: 'Processes', group: 'pc', icon: Activity, blurb: 'What uses the CPU, memory and GPU' },
  { id: 'sensors', label: 'Sensors', group: 'pc', icon: Thermometer, blurb: 'Every sensor, with min, max and average' },
  { id: 'widgets', label: 'Widgets', group: 'app', icon: LayoutGrid, blurb: 'Readings on your desktop' },
  { id: 'settings', label: 'Settings', group: 'app', icon: Settings, blurb: 'Look, profiles, overlay and more' },
] as const satisfies readonly View[];

export type Tab = (typeof VIEWS)[number]['id'];

export const isTab = (t: string | null | undefined): t is Tab => VIEWS.some((v) => v.id === t);

export const view = (id: Tab): View => VIEWS.find((v) => v.id === id)!;
