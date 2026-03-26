# Publish to GitHub

## 1) Configure identity (once)

git config --global user.name "Your Name"
git config --global user.email "you@example.com"

## 2) Create first commit

git add .
git commit -m "chore: bootstrap feishu-vscode-bridge open-source project"

## 3) Create an empty GitHub repo

Recommended name: feishu-vscode-bridge

## 4) Push

git branch -M main
git remote add origin https://github.com/<your-org>/feishu-vscode-bridge.git
git push -u origin main
