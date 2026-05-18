import * as vscode from 'vscode';
import { exec } from 'child_process';
import { promisify } from 'util';

const execAsync = promisify(exec);

export async function activate(context: vscode.ExtensionContext) {
    const outputChannel = vscode.window.createOutputChannel("Topos");
    
    async function checkToposInstalled(): Promise<boolean> {
        try {
            await execAsync('topos --version');
            return true;
        } catch (error) {
            return false;
        }
    }

    async function installTopos() {
        const terminal = vscode.window.createTerminal("Install Topos");
        terminal.show();
        terminal.sendText("curl -sSL https://raw.githubusercontent.com/Krv-Labs/topos/main/install.sh | sh");
        
        const response = await vscode.window.showInformationMessage(
            "Topos installation started in terminal. Please click 'Reload' once the installation is complete.",
            "Reload"
        );
        
        if (response === "Reload") {
            vscode.commands.executeCommand("workbench.action.reloadWindow");
        }
    }

    const isInstalled = await checkToposInstalled();

    if (!isInstalled) {
        const response = await vscode.window.showInformationMessage(
            "Topos CLI not found. It is required for the Topos Code Quality MCP extension.",
            "Install Topos"
        );
        if (response === "Install Topos") {
            await installTopos();
        }
        return;
    }

    // Register MCP Server Provider
    const providerId = 'topos-mcp';
    const provider: any = {
        provideMcpServerDefinitions: (token: vscode.CancellationToken) => {
            const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
            const env: Record<string, string> = {};
            
            if (workspaceRoot) {
                env['TOPOS_MCP_FILE_ROOT'] = workspaceRoot;
            }

            return [
                new (vscode as any).McpStdioServerDefinition(
                    "Topos Code Quality",
                    "topos",
                    ["mcp"],
                    env
                )
            ];
        }
    };

    context.subscriptions.push(
        (vscode as any).lm.registerMcpServerDefinitionProvider(providerId, provider)
    );

    outputChannel.appendLine("Topos MCP Server registered successfully.");
}

export function deactivate() {}
