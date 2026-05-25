// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
	integrations: [
		starlight({
			title: 'treeline',
			logo: {
				src: './src/assets/logo.svg',
			},
			social: [
				{ icon: 'github', label: 'GitHub', href: 'https://github.com/treeline-money/treeline' },
				{ icon: 'discord', label: 'Discord', href: 'https://discord.gg/EcNvBnSft5' },
			],
			customCss: ['./src/styles/custom.css'],
			components: {
				Head: './src/components/Head.astro',
			},
			sidebar: [
				{ label: 'Welcome', slug: 'index' },
				{
					label: 'Getting Started',
					items: [
						{ label: 'Installation', slug: 'getting-started/installation' },
						{ label: 'Quick Start', slug: 'getting-started/quick-start' },
						{ label: 'Importing Data', slug: 'getting-started/importing-data' },
					],
				},
				{
					label: 'AI & Agents',
					items: [
						{ label: 'Overview', slug: 'ai-agents' },
						{ label: 'MCP Server', slug: 'ai-agents/mcp-server' },
						{ label: 'OpenClaw', slug: 'ai-agents/openclaw' },
						{ label: 'Claude Code', slug: 'ai-agents/claude-code' },
						{ label: 'CLI for Agents', slug: 'ai-agents/cli-for-agents' },
						{ label: 'User Skills', slug: 'ai-agents/user-skills' },
						{ label: 'Example Workflows', slug: 'ai-agents/example-workflows' },
					],
				},
				{
					label: 'Desktop App',
					items: [
						{ label: 'Accounts', slug: 'desktop-app/accounts' },
						{ label: 'Transactions', slug: 'desktop-app/transactions' },
						{ label: 'Rules', slug: 'desktop-app/rules' },
						{ label: 'Query', slug: 'desktop-app/query' },
						{ label: 'Settings', slug: 'desktop-app/settings' },
					],
				},
				{ label: 'CLI', slug: 'cli' },
				{
					label: 'Plugins',
					items: [
						{ label: 'Overview', slug: 'plugins' },
						{ label: 'Creating Plugins', slug: 'plugins/creating-plugins' },
						{ label: 'SDK Reference', slug: 'plugins/sdk-reference' },
						{ label: 'Publishing', slug: 'plugins/publishing' },
					],
				},
				{
					label: 'Data Sources',
					items: [
						{ label: 'Bank Sync', slug: 'integrations/bank-sync' },
						{ label: 'CSV Import', slug: 'integrations/csv-import' },
					],
				},
				{
					label: 'Remote Access',
					badge: { text: 'Experimental', variant: 'caution' },
					items: [
						{ label: "What's a Hub?", slug: 'remote-access' },
						{ label: 'Run a Hub on Your Machine', slug: 'remote-access/on-your-machine' },
						{ label: 'Run an Always-On Hub', slug: 'remote-access/always-on' },
						{ label: 'Connect & Manage', slug: 'remote-access/connect' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'Database Schema', slug: 'reference/database-schema' },
						{ label: 'Data Location', slug: 'reference/data-location' },
					],
				},
				{ label: 'Contributing', slug: 'contributing' },
			],
		}),
	],
});
