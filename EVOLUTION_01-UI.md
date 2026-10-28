# evolutions de l'interface utilisateur

## bitchat-console

### gestion des messages DM

- les messages perdent de leur intéret en tant qu'échange lorsque l'un des intervenant à quitté la la cellule (critères: plus de présence, ni de message depuis un certain temps: ex 10mn)

- ils ne seront plus conservé que pour des raisons historiques et relecture ultérieures.

- ils seront concervés dans une base (sqlite) avec les clés geohash, timestamp, emetteur, desinataire et contenu.

- cette base sera maintenue avec les droits '600'

- une autre outil batch en permettra la gestion ultérieurement: probablement une IA pour en faire des analyses, synthèses, purges, etc...

- rédiger une api CRUD pour cette base et un SKILL.md pour cette api.

### gestion des messages de cellules

- les messages seront conservés dans la même base que les messages privés.

- cette base rera relue lors de la reselection d'une cellule 

- tous les messages correspondant à la cellule seront affichés sur demande dans un nouvel onglet du navigateur 

- dans cet onglet sera présentée une interface permettant la recherche, la selection, le tri, l'effacement.

- une IA pourra être ultérieurement connecté à cette base y faire de la modération, des analyses et autres traitements.

- les modification extérieure à cette base (par exemple par l'IA) devront être répercutée dans l'interface graphique dès que possible.

les messages affichés seront grisés (mais lisibles) lorsque les participants auront quittés la cellule (voir critères).

### nickname

- il doit être mémorisé et réutilisé dans sa dernière version lors du re-lancement de l'application

- il doit être utilisé pour les messages vers la cellule, les messages privés et les méssages pré-enregistrés (saved messages)


### messages privés

- prévoir une méthode pour que l'utilisateur puisse les effacer 

- ne plus afficher dans la console les messages privés dont les intervenant ont quité l'échange (voir le critère dans gestion des messages DM)

- comportement de la zone de saisie à revoir, on ne voit plus ce que l'on écrit une fois arrivé au bout de la zone (scoll manquant)

- idem pour la zone de saisie de message aux intervenants de la cellule