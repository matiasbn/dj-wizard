añade una nueva funcionalidad a dj-wizard que me permita seguir los tracks disponibles de un genero en particular
https://soundeo.com/list/tracks?availableFilter=1&genreFilter=1&timeFilter=r_2020-09-02_2025-09-15&page=1
en ese link voy a encontrar todas las canciones disponibles (availableFilter=1), del genero 1 (Drum and Bass) en el timefilter particular, desde la pagina 1
la nueva funcionalidad debe permitirme añadir un nuevo genero, o seleccionar generos que ya tenga guardados
el link guardado va a dar el rango de fechas en que quiero buscar en soundeo
al entrar a ese link, tendré todas las canciones disponibles en la plataforma de ese genero en particular en esa fecha
lo que quiero que ocurra cuando escoja un genero que ya este guardado, es que genere un nuevo link con la ultima fecha que se reviso el link (inclusive) hasta la fecha actual
se debe generar un registro nuevo en el json que tiene los datos que guarde esta data para poder correrlo de nuevo en el futuro
despues quiero que añada todos los tracks que devuelva ese link a la cola
esto es desde la pagina 1, avannzando pagina por apgina, añadiendo los tracks a la cola con prioridad standard, hasta obtener un error 404 (que significa que se acabaron las paginas)
el script debe saltarse las canciones que ya han sido agregadas y las que ya han sido descargadas
explicame una estrategia para implementar esta nueva funcionalidad
