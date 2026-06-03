import { defineConfig } from '@vscode/test-cli';

export default defineConfig({
    files: 'out/test/integration/**/*.test.js',
    version: 'stable',
    mocha: {
        timeout: 60000,
    },
});
