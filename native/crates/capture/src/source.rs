/// Uma tela inteira disponível para captura.
#[derive(Debug, Clone)]
pub struct Display {
    pub id: u32,
    pub width: u32,
    pub height: u32,
}

/// Uma janela específica. Compartilhar janela evita mostrar o que não devia.
#[derive(Debug, Clone)]
pub struct Window {
    pub id: u32,
    pub title: String,
    pub application: String,
}
