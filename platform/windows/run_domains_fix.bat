@echo off
cd /d E:\AllinDeepSeek\taishen-dict
python -u -c "from sources import wiki; import yaml, os; cfg=yaml.safe_load(open('domains.yaml','r',encoding='utf-8'))['domains']; [print(f'{n}: {len(wiki.collect_domain(n,cfg[n][\"wiki_categories\"],cfg[n].get(\"max_depth\",2)))} entries') for n in ['sport','food']]"
echo EXIT=%ERRORLEVEL%
