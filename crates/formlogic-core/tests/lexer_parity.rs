use formlogic_core::config::FormLogicConfig;
use formlogic_core::lexer::Lexer;
use formlogic_core::token::TokenType;

#[test]
fn lexer_basic_tokens_parity_subset() {
    let mut lexer = Lexer::new("42", FormLogicConfig::default());
    let token = lexer.next_token();
    assert_eq!(token.token_type, TokenType::Int);
    assert_eq!(token.literal, "42");

    let mut lexer = Lexer::new("3.14", FormLogicConfig::default());
    let token = lexer.next_token();
    assert_eq!(token.token_type, TokenType::Float);
    assert_eq!(token.literal, "3.14");

    let mut lexer = Lexer::new("1e3", FormLogicConfig::default());
    let token = lexer.next_token();
    assert_eq!(token.token_type, TokenType::Float);
    assert_eq!(token.literal, "1e3");

    let mut lexer = Lexer::new("0xff", FormLogicConfig::default());
    let token = lexer.next_token();
    assert_eq!(token.token_type, TokenType::Int);
    assert_eq!(token.literal, "0xff");

    let mut lexer = Lexer::new("0b101", FormLogicConfig::default());
    let token = lexer.next_token();
    assert_eq!(token.token_type, TokenType::Int);
    assert_eq!(token.literal, "0b101");

    let mut lexer = Lexer::new("0o10", FormLogicConfig::default());
    let token = lexer.next_token();
    assert_eq!(token.token_type, TokenType::Int);
    assert_eq!(token.literal, "0o10");

    let mut lexer = Lexer::new("\"hello world\"", FormLogicConfig::default());
    let token = lexer.next_token();
    assert_eq!(token.token_type, TokenType::String);
    assert_eq!(token.literal, "hello world");

    let mut lexer = Lexer::new("`hello`", FormLogicConfig::default());
    let token = lexer.next_token();
    assert_eq!(token.token_type, TokenType::Template);
    assert_eq!(token.literal, "hello");
}

#[test]
fn lexer_keywords_parity_subset() {
    let keywords = [
        ("let", TokenType::Let),
        ("const", TokenType::Const),
        ("function", TokenType::Function),
        ("return", TokenType::Return),
        ("if", TokenType::If),
        ("else", TokenType::Else),
        ("true", TokenType::True),
        ("false", TokenType::False),
        ("null", TokenType::Null),
        ("undefined", TokenType::Undefined),
        ("while", TokenType::While),
        ("for", TokenType::For),
        ("class", TokenType::Class),
        ("extends", TokenType::Extends),
        ("new", TokenType::New),
        ("this", TokenType::This),
        ("super", TokenType::Super),
        ("try", TokenType::Try),
        ("catch", TokenType::Catch),
        ("throw", TokenType::Throw),
        ("finally", TokenType::Finally),
        ("async", TokenType::Async),
        ("await", TokenType::Await),
    ];

    for (kw, expected) in keywords {
        let mut lexer = Lexer::new(kw, FormLogicConfig::default());
        let token = lexer.next_token();
        assert_eq!(token.token_type, expected);
        assert_eq!(token.literal, kw);
    }
}

#[test]
fn lexer_operators_and_delimiters_parity_subset() {
    let input = "+ - * / % < > ! = & | == != <= >= && || => ++ -- += -= *= /= === !== ( ) { } [ ] , ; : . ? ...";
    let expected = [
        TokenType::Plus,
        TokenType::Minus,
        TokenType::Asterisk,
        TokenType::Slash,
        TokenType::Percent,
        TokenType::LessThan,
        TokenType::GreaterThan,
        TokenType::Bang,
        TokenType::Assign,
        TokenType::BitwiseAnd,
        TokenType::BitwiseOr,
        TokenType::Equal,
        TokenType::NotEqual,
        TokenType::LessThanOrEqual,
        TokenType::GreaterThanOrEqual,
        TokenType::And,
        TokenType::Or,
        TokenType::Arrow,
        TokenType::Increment,
        TokenType::Decrement,
        TokenType::PlusAssign,
        TokenType::MinusAssign,
        TokenType::MultiplyAssign,
        TokenType::DivideAssign,
        TokenType::StrictEqual,
        TokenType::StrictNotEqual,
        TokenType::LeftParen,
        TokenType::RightParen,
        TokenType::LeftBrace,
        TokenType::RightBrace,
        TokenType::LeftBracket,
        TokenType::RightBracket,
        TokenType::Comma,
        TokenType::Semicolon,
        TokenType::Colon,
        TokenType::Dot,
        TokenType::Question,
        TokenType::Spread,
    ];

    let mut lexer = Lexer::new(input, FormLogicConfig::default());
    for expected_type in expected {
        let token = lexer.next_token();
        assert_eq!(token.token_type, expected_type);
    }
}

#[test]
fn lexer_comments_whitespace_and_positions_parity_subset() {
    let input = "\n// comment\nlet x = 5;\n/* block */ let y = 10;\n";
    let mut lexer = Lexer::new(input, FormLogicConfig::default());

    let t1 = lexer.next_token();
    assert_eq!(t1.token_type, TokenType::Let);
    assert_eq!(t1.line, 3);

    assert_eq!(lexer.next_token().token_type, TokenType::Ident); // x
    assert_eq!(lexer.next_token().token_type, TokenType::Assign);
    assert_eq!(lexer.next_token().token_type, TokenType::Int);
    assert_eq!(lexer.next_token().token_type, TokenType::Semicolon);

    let t2 = lexer.next_token();
    assert_eq!(t2.token_type, TokenType::Let);
    assert_eq!(t2.literal, "let");
}

#[test]
fn lexer_escape_and_eof_parity_subset() {
    let mut lexer = Lexer::new("\"hello\\nworld\"", FormLogicConfig::default());
    let token = lexer.next_token();
    assert_eq!(token.token_type, TokenType::String);
    assert_eq!(token.literal, "hello\nworld");

    let mut lexer = Lexer::new("42", FormLogicConfig::default());
    let _ = lexer.next_token();
    assert_eq!(lexer.next_token().token_type, TokenType::Eof);
    assert_eq!(lexer.next_token().token_type, TokenType::Eof);
}
