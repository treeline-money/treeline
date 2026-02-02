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
					label: 'Integrations',
					items: [
						{ label: 'Bank Sync', slug: 'integrations/bank-sync' },
						{ label: 'CSV Import', slug: 'integrations/csv-import' },
						{ label: 'OpenClaw', slug: 'integrations/openclaw' },
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
