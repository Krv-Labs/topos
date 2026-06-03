import * as assert from 'assert';
import * as vscode from 'vscode';

const EXTENSION_ID = 'KrvLabs.topos-vscode';

suite('Topos extension activation', () => {
    test('extension is present', () => {
        assert.ok(vscode.extensions.getExtension(EXTENSION_ID), `Extension ${EXTENSION_ID} not found`);
    });

    test('activates without throwing', async () => {
        const extension = vscode.extensions.getExtension(EXTENSION_ID);
        assert.ok(extension);
        await extension!.activate();
        assert.strictEqual(extension!.isActive, true);
    });

    test('registers the Generate Dependency Graph command', async () => {
        const extension = vscode.extensions.getExtension(EXTENSION_ID);
        await extension!.activate();
        const commands = await vscode.commands.getCommands(true);
        assert.ok(
            commands.includes('topos.generateDependencyGraph'),
            'topos.generateDependencyGraph command was not registered'
        );
        assert.ok(
            commands.includes('topos.evaluateProject'),
            'topos.evaluateProject command was not registered'
        );
    });

    test('host exposes the MCP server definition API', () => {
        assert.strictEqual(
            typeof (vscode as any).lm?.registerMcpServerDefinitionProvider,
            'function',
            'vscode.lm.registerMcpServerDefinitionProvider missing in test host'
        );
    });
});
