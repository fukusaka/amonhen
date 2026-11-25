#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Block(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Block, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(Block, TextElement, DependencyObject);
impl Block {
    pub fn ContentStart(&self) -> windows_core::Result<TextPointer> {
        let this = &windows_core::Interface::cast::<ITextElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContentStart)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    fn IBlockFactory<R, F: FnOnce(&IBlockFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Block, IBlockFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IBlockStatics<R, F: FnOnce(&IBlockStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Block, IBlockStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Block {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IBlock>();
}
unsafe impl windows_core::Interface for Block {
    type Vtable = <IBlock as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IBlock as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for Block {
    const NAME: &'static str = "Microsoft.UI.Xaml.Documents.Block";
}
unsafe impl Send for Block {}
unsafe impl Sync for Block {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockCollection(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    BlockCollection,
    windows_core::IUnknown,
    windows_core::IInspectable,
    windows_collections::IVector<Block>
);
impl windows_core::RuntimeType for BlockCollection {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, windows_collections::IVector<Block>>();
}
unsafe impl windows_core::Interface for BlockCollection {
    type Vtable = <windows_collections::IVector<Block> as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <windows_collections::IVector<Block> as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for BlockCollection {
    const NAME: &'static str = "Microsoft.UI.Xaml.Documents.BlockCollection";
}
unsafe impl Send for BlockCollection {}
unsafe impl Sync for BlockCollection {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Border(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Border, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(Border, FrameworkElement, UIElement, DependencyObject);
impl Border {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Border, windows_core::imp::IGenericFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn SetBorderBrush<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Brush>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetBorderBrush)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetBorderThickness(&self, value: Thickness) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetBorderThickness)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetBackground<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Brush>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetBackground)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetCornerRadius(&self, value: CornerRadius) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetCornerRadius)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetPadding(&self, value: Thickness) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetPadding)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetChild<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<UIElement>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetChild)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn IBorderStatics<R, F: FnOnce(&IBorderStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Border, IBorderStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Border {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IBorder>();
}
unsafe impl windows_core::Interface for Border {
    type Vtable = <IBorder as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IBorder as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for Border {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Border";
}
unsafe impl Send for Border {}
unsafe impl Sync for Border {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BringIntoViewOptions(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    BringIntoViewOptions,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl BringIntoViewOptions {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            BringIntoViewOptions,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn SetTargetRect(&self, value: Option<Rect>) -> windows_core::Result<()> {
        let value__ = value.map(<windows_reference::IReference<Rect> as From<_>>::from);
        unsafe {
            (windows_core::Interface::vtable(self).SetTargetRect)(
                windows_core::Interface::as_raw(self),
                windows_core::Param::param(value__.as_ref()).abi(),
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignmentRatio(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetVerticalAlignmentRatio)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
impl windows_core::RuntimeType for BringIntoViewOptions {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IBringIntoViewOptions>();
}
unsafe impl windows_core::Interface for BringIntoViewOptions {
    type Vtable = <IBringIntoViewOptions as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IBringIntoViewOptions as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for BringIntoViewOptions {
    const NAME: &'static str = "Microsoft.UI.Xaml.BringIntoViewOptions";
}
unsafe impl Send for BringIntoViewOptions {}
unsafe impl Sync for BringIntoViewOptions {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Brush(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Brush, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(Brush, DependencyObject);
impl Brush {
    fn IBrushFactory<R, F: FnOnce(&IBrushFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Brush, IBrushFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IBrushStatics<R, F: FnOnce(&IBrushStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Brush, IBrushStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Brush {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IBrush>();
}
unsafe impl windows_core::Interface for Brush {
    type Vtable = <IBrush as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IBrush as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for Brush {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.Brush";
}
unsafe impl Send for Brush {}
unsafe impl Sync for Brush {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Button(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Button, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(
    Button,
    ButtonBase,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl Button {
    pub fn Flyout(&self) -> windows_core::Result<FlyoutBase> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Flyout)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetFlyout)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Content(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Content)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContent)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetContentTemplate<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DataTemplate>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContentTemplate)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn IButtonFactory<R, F: FnOnce(&IButtonFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Button, IButtonFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IButtonStatics<R, F: FnOnce(&IButtonStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Button, IButtonStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Button {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IButton>();
}
unsafe impl windows_core::Interface for Button {
    type Vtable = <IButton as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IButton as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for Button {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Button";
}
unsafe impl Send for Button {}
unsafe impl Sync for Button {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ButtonBase(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ButtonBase,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ButtonBase,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl ButtonBase {
    pub fn Content(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Content)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContent)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetContentTemplate<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DataTemplate>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContentTemplate)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn IButtonBaseFactory<R, F: FnOnce(&IButtonBaseFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ButtonBase, IButtonBaseFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IButtonBaseStatics<R, F: FnOnce(&IButtonBaseStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ButtonBase, IButtonBaseStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ButtonBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IButtonBase>();
}
unsafe impl windows_core::Interface for ButtonBase {
    type Vtable = <IButtonBase as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IButtonBase as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for ButtonBase {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.ButtonBase";
}
unsafe impl Send for ButtonBase {}
unsafe impl Sync for ButtonBase {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Color {
    pub A: u8,
    pub R: u8,
    pub G: u8,
    pub B: u8,
}
impl windows_core::imp::TypeKind for Color {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for Color {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"struct(Windows.UI.Color;u1;u1;u1;u1)");
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Windows.UI.Color");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionBrush(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CompositionBrush,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(CompositionBrush, CompositionObject);
impl windows_core::RuntimeType for CompositionBrush {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICompositionBrush>();
}
unsafe impl windows_core::Interface for CompositionBrush {
    type Vtable = <ICompositionBrush as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ICompositionBrush as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for CompositionBrush {
    const NAME: &'static str = "Windows.UI.Composition.CompositionBrush";
}
unsafe impl Send for CompositionBrush {}
unsafe impl Sync for CompositionBrush {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionObject(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CompositionObject,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl CompositionObject {
    fn ICompositionObjectStatics<
        R,
        F: FnOnce(&ICompositionObjectStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            CompositionObject,
            ICompositionObjectStatics,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for CompositionObject {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICompositionObject>();
}
unsafe impl windows_core::Interface for CompositionObject {
    type Vtable = <ICompositionObject as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ICompositionObject as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for CompositionObject {
    const NAME: &'static str = "Windows.UI.Composition.CompositionObject";
}
unsafe impl Send for CompositionObject {}
unsafe impl Sync for CompositionObject {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentControl(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ContentControl,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl ContentControl {
    pub fn Content(&self) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Content)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetContent)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetContentTemplate<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DataTemplate>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetContentTemplate)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn IContentControlFactory<R, F: FnOnce(&IContentControlFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ContentControl, IContentControlFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IContentControlStatics<R, F: FnOnce(&IContentControlStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ContentControl, IContentControlStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ContentControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IContentControl>();
}
unsafe impl windows_core::Interface for ContentControl {
    type Vtable = <IContentControl as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IContentControl as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for ContentControl {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ContentControl";
}
unsafe impl Send for ContentControl {}
unsafe impl Sync for ContentControl {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentDialog(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ContentDialog,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ContentDialog,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl ContentDialog {
    pub fn Content(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Content)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContent)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetContentTemplate<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DataTemplate>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContentTemplate)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetDefaultButton(&self, value: ContentDialogButton) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetDefaultButton)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn IContentDialogFactory<R, F: FnOnce(&IContentDialogFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ContentDialog, IContentDialogFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IContentDialogStatics<R, F: FnOnce(&IContentDialogStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ContentDialog, IContentDialogStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ContentDialog {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IContentDialog>();
}
unsafe impl windows_core::Interface for ContentDialog {
    type Vtable = <IContentDialog as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IContentDialog as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for ContentDialog {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ContentDialog";
}
unsafe impl Send for ContentDialog {}
unsafe impl Sync for ContentDialog {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContentDialogButton(pub i32);
impl ContentDialogButton {
    pub const None: Self = Self(0);
    pub const Primary: Self = Self(1);
    pub const Secondary: Self = Self(2);
    pub const Close: Self = Self(3);
}
impl windows_core::imp::TypeKind for ContentDialogButton {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for ContentDialogButton {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Xaml.Controls.ContentDialogButton;i4)",
    );
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.ContentDialogButton",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentIslandEnvironment(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ContentIslandEnvironment,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl ContentIslandEnvironment {
    pub fn AppWindowId(&self) -> windows_core::Result<WindowId> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).AppWindowId)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
impl windows_core::RuntimeType for ContentIslandEnvironment {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IContentIslandEnvironment>();
}
unsafe impl windows_core::Interface for ContentIslandEnvironment {
    type Vtable = <IContentIslandEnvironment as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IContentIslandEnvironment as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for ContentIslandEnvironment {
    const NAME: &'static str = "Microsoft.UI.Content.ContentIslandEnvironment";
}
unsafe impl Send for ContentIslandEnvironment {}
unsafe impl Sync for ContentIslandEnvironment {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextRequestedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ContextRequestedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(ContextRequestedEventArgs, RoutedEventArgs);
impl ContextRequestedEventArgs {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            ContextRequestedEventArgs,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn Handled(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Handled)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetHandled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHandled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn TryGetPosition<P0>(
        &self,
        relativeto: P0,
        point: &mut Point,
    ) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<UIElement>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).TryGetPosition)(
                windows_core::Interface::as_raw(self),
                relativeto.param().abi(),
                point,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn OriginalSource(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IRoutedEventArgs>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).OriginalSource)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for ContextRequestedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IContextRequestedEventArgs>();
}
unsafe impl windows_core::Interface for ContextRequestedEventArgs {
    type Vtable = <IContextRequestedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IContextRequestedEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for ContextRequestedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Input.ContextRequestedEventArgs";
}
unsafe impl Send for ContextRequestedEventArgs {}
unsafe impl Sync for ContextRequestedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Control(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Control,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(Control, FrameworkElement, UIElement, DependencyObject);
impl Control {
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetFontFamily)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn IControlFactory<R, F: FnOnce(&IControlFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Control, IControlFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IControlStatics<R, F: FnOnce(&IControlStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Control, IControlStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Control {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IControl>();
}
unsafe impl windows_core::Interface for Control {
    type Vtable = <IControl as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IControl as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for Control {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Control";
}
unsafe impl Send for Control {}
unsafe impl Sync for Control {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreVirtualKeyStates(pub u32);
impl CoreVirtualKeyStates {
    pub const None: Self = Self(0);
    pub const Down: Self = Self(1);
    pub const Locked: Self = Self(2);
}
impl windows_core::imp::TypeKind for CoreVirtualKeyStates {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreVirtualKeyStates {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Windows.UI.Core.CoreVirtualKeyStates;u4)",
    );
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Windows.UI.Core.CoreVirtualKeyStates");
}
impl CoreVirtualKeyStates {
    pub const fn contains(&self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}
impl core::ops::BitOr for CoreVirtualKeyStates {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}
impl core::ops::BitAnd for CoreVirtualKeyStates {
    type Output = Self;
    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}
impl core::ops::BitOrAssign for CoreVirtualKeyStates {
    fn bitor_assign(&mut self, other: Self) {
        self.0.bitor_assign(other.0);
    }
}
impl core::ops::BitAndAssign for CoreVirtualKeyStates {
    fn bitand_assign(&mut self, other: Self) {
        self.0.bitand_assign(other.0);
    }
}
impl core::ops::Not for CoreVirtualKeyStates {
    type Output = Self;
    fn not(self) -> Self {
        Self(self.0.not())
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadius {
    pub TopLeft: f64,
    pub TopRight: f64,
    pub BottomRight: f64,
    pub BottomLeft: f64,
}
impl windows_core::imp::TypeKind for CornerRadius {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CornerRadius {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"struct(Microsoft.UI.Xaml.CornerRadius;f8;f8;f8;f8)",
    );
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.CornerRadius");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataPackage(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DataPackage,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl DataPackage {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            DataPackage,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn SetRequestedOperation(&self, value: DataPackageOperation) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetRequestedOperation)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetText(&self, value: &windows_core::HSTRING) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetText)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(value),
            )
            .ok()
        }
    }
}
impl windows_core::RuntimeType for DataPackage {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDataPackage>();
}
unsafe impl windows_core::Interface for DataPackage {
    type Vtable = <IDataPackage as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDataPackage as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for DataPackage {
    const NAME: &'static str = "Windows.ApplicationModel.DataTransfer.DataPackage";
}
unsafe impl Send for DataPackage {}
unsafe impl Sync for DataPackage {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DataPackageOperation(pub u32);
impl DataPackageOperation {
    pub const None: Self = Self(0);
    pub const Copy: Self = Self(1);
    pub const Move: Self = Self(2);
    pub const Link: Self = Self(4);
    pub const NewTarget: Self = Self(1073741824);
    pub const BackgroundTarget: Self = Self(536870912);
}
impl windows_core::imp::TypeKind for DataPackageOperation {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for DataPackageOperation {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Windows.ApplicationModel.DataTransfer.DataPackageOperation;u4)",
    );
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Windows.ApplicationModel.DataTransfer.DataPackageOperation",
    );
}
impl DataPackageOperation {
    pub const fn contains(&self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}
impl core::ops::BitOr for DataPackageOperation {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}
impl core::ops::BitAnd for DataPackageOperation {
    type Output = Self;
    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}
impl core::ops::BitOrAssign for DataPackageOperation {
    fn bitor_assign(&mut self, other: Self) {
        self.0.bitor_assign(other.0);
    }
}
impl core::ops::BitAndAssign for DataPackageOperation {
    fn bitand_assign(&mut self, other: Self) {
        self.0.bitand_assign(other.0);
    }
}
impl core::ops::Not for DataPackageOperation {
    type Output = Self;
    fn not(self) -> Self {
        Self(self.0.not())
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataTemplate(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DataTemplate,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(DataTemplate, FrameworkTemplate, DependencyObject);
impl DataTemplate {
    fn IDataTemplateFactory<R, F: FnOnce(&IDataTemplateFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<DataTemplate, IDataTemplateFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IDataTemplateStatics<R, F: FnOnce(&IDataTemplateStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<DataTemplate, IDataTemplateStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for DataTemplate {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDataTemplate>();
}
unsafe impl windows_core::Interface for DataTemplate {
    type Vtable = <IDataTemplate as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDataTemplate as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for DataTemplate {
    const NAME: &'static str = "Microsoft.UI.Xaml.DataTemplate";
}
unsafe impl Send for DataTemplate {}
unsafe impl Sync for DataTemplate {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyObject(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DependencyObject,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl DependencyObject {
    fn IDependencyObjectFactory<
        R,
        F: FnOnce(&IDependencyObjectFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<DependencyObject, IDependencyObjectFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for DependencyObject {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDependencyObject>();
}
unsafe impl windows_core::Interface for DependencyObject {
    type Vtable = <IDependencyObject as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDependencyObject as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for DependencyObject {
    const NAME: &'static str = "Microsoft.UI.Xaml.DependencyObject";
}
unsafe impl Send for DependencyObject {}
unsafe impl Sync for DependencyObject {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopAcrylicController(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DesktopAcrylicController,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    DesktopAcrylicController,
    ISystemBackdropControllerWithTargets
);
impl DesktopAcrylicController {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            DesktopAcrylicController,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn SetLuminosityOpacity(&self, value: f32) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetLuminosityOpacity)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetTintOpacity(&self, value: f32) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetTintOpacity)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn AddSystemBackdropTarget<P0>(
        &self,
        systembackdroptarget: P0,
    ) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<ICompositionSupportsSystemBackdrop>,
    {
        let this = &windows_core::Interface::cast::<ISystemBackdropControllerWithTargets>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).AddSystemBackdropTarget)(
                windows_core::Interface::as_raw(this),
                systembackdroptarget.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn RemoveAllSystemBackdropTargets(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<ISystemBackdropControllerWithTargets>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).RemoveAllSystemBackdropTargets)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn SetSystemBackdropConfiguration<P0>(&self, configuration: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<SystemBackdropConfiguration>,
    {
        let this = &windows_core::Interface::cast::<ISystemBackdropControllerWithTargets>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetSystemBackdropConfiguration)(
                windows_core::Interface::as_raw(this),
                configuration.param().abi(),
            )
            .ok()
        }
    }
    fn IDesktopAcrylicControllerStatics<
        R,
        F: FnOnce(&IDesktopAcrylicControllerStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            DesktopAcrylicController,
            IDesktopAcrylicControllerStatics,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for DesktopAcrylicController {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDesktopAcrylicController>();
}
unsafe impl windows_core::Interface for DesktopAcrylicController {
    type Vtable = <IDesktopAcrylicController as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDesktopAcrylicController as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for DesktopAcrylicController {
    const NAME: &'static str = "Microsoft.UI.Composition.SystemBackdrops.DesktopAcrylicController";
}
unsafe impl Send for DesktopAcrylicController {}
unsafe impl Sync for DesktopAcrylicController {}
windows_core::imp::define_interface!(
    DoubleTappedEventHandler,
    DoubleTappedEventHandler_Vtbl,
    0xf7a501b9_e277_5611_87b0_0e0607622183
);
impl windows_core::RuntimeType for DoubleTappedEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl DoubleTappedEventHandler {
    pub fn new<
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) -> windows_core::Result<()>
            + Send
            + 'static,
    >(
        invoke: F,
    ) -> Self {
        let com = windows_core::imp::DelegateBox::<Self, F>::new(
            &DoubleTappedEventHandlerBox::<F>::VTABLE,
            invoke,
        );
        unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
    }
    pub fn Invoke<P0, P1>(&self, sender: P0, e: P1) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
        P1: windows_core::Param<DoubleTappedRoutedEventArgs>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).Invoke)(
                windows_core::Interface::as_raw(self),
                sender.param().abi(),
                e.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct DoubleTappedEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct DoubleTappedEventHandlerBox<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<DoubleTappedRoutedEventArgs>,
        ) -> windows_core::Result<()>
        + Send
        + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<DoubleTappedRoutedEventArgs>,
        ) -> windows_core::Result<()>
        + Send
        + 'static,
> DoubleTappedEventHandlerBox<F>
{
    const VTABLE: DoubleTappedEventHandler_Vtbl = DoubleTappedEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface:
                windows_core::imp::DelegateBox::<DoubleTappedEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<DoubleTappedEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<DoubleTappedEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<DoubleTappedEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            )
            .into()
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DoubleTappedRoutedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DoubleTappedRoutedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(DoubleTappedRoutedEventArgs, RoutedEventArgs);
impl DoubleTappedRoutedEventArgs {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            DoubleTappedRoutedEventArgs,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn Handled(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Handled)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetHandled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHandled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn GetPosition<P0>(&self, relativeto: P0) -> windows_core::Result<Point>
    where
        P0: windows_core::Param<UIElement>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetPosition)(
                windows_core::Interface::as_raw(self),
                relativeto.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn OriginalSource(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IRoutedEventArgs>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).OriginalSource)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for DoubleTappedRoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDoubleTappedRoutedEventArgs>();
}
unsafe impl windows_core::Interface for DoubleTappedRoutedEventArgs {
    type Vtable = <IDoubleTappedRoutedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDoubleTappedRoutedEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for DoubleTappedRoutedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs";
}
unsafe impl Send for DoubleTappedRoutedEventArgs {}
unsafe impl Sync for DoubleTappedRoutedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DragEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DragEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(DragEventArgs, RoutedEventArgs);
impl DragEventArgs {
    pub fn Handled(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Handled)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetHandled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHandled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn AcceptedOperation(&self) -> windows_core::Result<DataPackageOperation> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).AcceptedOperation)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetAcceptedOperation(&self, value: DataPackageOperation) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetAcceptedOperation)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn GetPosition<P0>(&self, relativeto: P0) -> windows_core::Result<Point>
    where
        P0: windows_core::Param<UIElement>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetPosition)(
                windows_core::Interface::as_raw(self),
                relativeto.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn OriginalSource(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IRoutedEventArgs>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).OriginalSource)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for DragEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDragEventArgs>();
}
unsafe impl windows_core::Interface for DragEventArgs {
    type Vtable = <IDragEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDragEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for DragEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.DragEventArgs";
}
unsafe impl Send for DragEventArgs {}
unsafe impl Sync for DragEventArgs {}
windows_core::imp::define_interface!(
    DragEventHandler,
    DragEventHandler_Vtbl,
    0x277afc83_cb67_56c8_b601_1b9c0f1c3d32
);
impl windows_core::RuntimeType for DragEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl DragEventHandler {
    pub fn new<
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DragEventArgs>,
            ) -> windows_core::Result<()>
            + Send
            + 'static,
    >(
        invoke: F,
    ) -> Self {
        let com = windows_core::imp::DelegateBox::<Self, F>::new(
            &DragEventHandlerBox::<F>::VTABLE,
            invoke,
        );
        unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
    }
    pub fn Invoke<P0, P1>(&self, sender: P0, e: P1) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
        P1: windows_core::Param<DragEventArgs>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).Invoke)(
                windows_core::Interface::as_raw(self),
                sender.param().abi(),
                e.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct DragEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct DragEventHandlerBox<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<DragEventArgs>,
        ) -> windows_core::Result<()>
        + Send
        + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<DragEventArgs>,
        ) -> windows_core::Result<()>
        + Send
        + 'static,
> DragEventHandlerBox<F>
{
    const VTABLE: DragEventHandler_Vtbl = DragEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface: windows_core::imp::DelegateBox::<DragEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<DragEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<DragEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<DragEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            )
            .into()
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DragItemsStartingEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DragItemsStartingEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl DragItemsStartingEventArgs {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            DragItemsStartingEventArgs,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn Cancel(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Cancel)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetCancel(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetCancel)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Items(
        &self,
    ) -> windows_core::Result<windows_collections::IVector<windows_core::IInspectable>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Items)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn Data(&self) -> windows_core::Result<DataPackage> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Data)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for DragItemsStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDragItemsStartingEventArgs>();
}
unsafe impl windows_core::Interface for DragItemsStartingEventArgs {
    type Vtable = <IDragItemsStartingEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDragItemsStartingEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for DragItemsStartingEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.DragItemsStartingEventArgs";
}
unsafe impl Send for DragItemsStartingEventArgs {}
unsafe impl Sync for DragItemsStartingEventArgs {}
windows_core::imp::define_interface!(
    DragItemsStartingEventHandler,
    DragItemsStartingEventHandler_Vtbl,
    0x55532800_7617_5d67_80bf_b98c0a41b9d6
);
impl windows_core::RuntimeType for DragItemsStartingEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl DragItemsStartingEventHandler {
    pub fn new<
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DragItemsStartingEventArgs>,
            ) -> windows_core::Result<()>
            + Send
            + 'static,
    >(
        invoke: F,
    ) -> Self {
        let com = windows_core::imp::DelegateBox::<Self, F>::new(
            &DragItemsStartingEventHandlerBox::<F>::VTABLE,
            invoke,
        );
        unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
    }
    pub fn Invoke<P0, P1>(&self, sender: P0, e: P1) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
        P1: windows_core::Param<DragItemsStartingEventArgs>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).Invoke)(
                windows_core::Interface::as_raw(self),
                sender.param().abi(),
                e.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct DragItemsStartingEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct DragItemsStartingEventHandlerBox<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<DragItemsStartingEventArgs>,
        ) -> windows_core::Result<()>
        + Send
        + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<DragItemsStartingEventArgs>,
        ) -> windows_core::Result<()>
        + Send
        + 'static,
> DragItemsStartingEventHandlerBox<F>
{
    const VTABLE: DragItemsStartingEventHandler_Vtbl = DragItemsStartingEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface:
                windows_core::imp::DelegateBox::<DragItemsStartingEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<DragItemsStartingEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<DragItemsStartingEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<DragItemsStartingEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            )
            .into()
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ElementTheme(pub i32);
impl ElementTheme {
    pub const Default: Self = Self(0);
    pub const Light: Self = Self(1);
    pub const Dark: Self = Self(2);
}
impl windows_core::imp::TypeKind for ElementTheme {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for ElementTheme {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Microsoft.UI.Xaml.ElementTheme;i4)");
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.ElementTheme");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlyoutBase(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FlyoutBase,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(FlyoutBase, DependencyObject);
impl FlyoutBase {
    pub fn ShowAt<P0>(&self, placementtarget: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FrameworkElement>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ShowAt)(
                windows_core::Interface::as_raw(self),
                placementtarget.param().abi(),
            )
            .ok()
        }
    }
    pub fn ShowAtWithOptions<P0, P1>(
        &self,
        placementtarget: P0,
        showoptions: P1,
    ) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DependencyObject>,
        P1: windows_core::Param<FlyoutShowOptions>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ShowAtWithOptions)(
                windows_core::Interface::as_raw(self),
                placementtarget.param().abi(),
                showoptions.param().abi(),
            )
            .ok()
        }
    }
    pub fn Hide(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Hide)(windows_core::Interface::as_raw(self)).ok()
        }
    }
    fn IFlyoutBaseFactory<R, F: FnOnce(&IFlyoutBaseFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<FlyoutBase, IFlyoutBaseFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IFlyoutBaseStatics<R, F: FnOnce(&IFlyoutBaseStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<FlyoutBase, IFlyoutBaseStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IFlyoutBaseStatics2<R, F: FnOnce(&IFlyoutBaseStatics2) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<FlyoutBase, IFlyoutBaseStatics2> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for FlyoutBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFlyoutBase>();
}
unsafe impl windows_core::Interface for FlyoutBase {
    type Vtable = <IFlyoutBase as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFlyoutBase as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for FlyoutBase {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.FlyoutBase";
}
unsafe impl Send for FlyoutBase {}
unsafe impl Sync for FlyoutBase {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlyoutShowOptions(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FlyoutShowOptions,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl FlyoutShowOptions {
    pub fn SetPosition(&self, value: Option<Point>) -> windows_core::Result<()> {
        let value__ = value.map(<windows_reference::IReference<Point> as From<_>>::from);
        unsafe {
            (windows_core::Interface::vtable(self).SetPosition)(
                windows_core::Interface::as_raw(self),
                windows_core::Param::param(value__.as_ref()).abi(),
            )
            .ok()
        }
    }
    pub fn new() -> windows_core::Result<Self> {
        Self::IFlyoutShowOptionsFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    pub fn compose<T>(compose: T) -> windows_core::Result<Self>
    where
        T: windows_core::Compose,
    {
        Self::IFlyoutShowOptionsFactory(|this| unsafe {
            let (derived__, base__) = windows_core::Compose::compose(compose);
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(&derived__),
                base__ as *mut _ as _,
                &mut result__,
            )
            .ok()?;
            let _ = &derived__;
            windows_core::imp::Type::from_abi(result__)
        })
    }
    fn IFlyoutShowOptionsFactory<
        R,
        F: FnOnce(&IFlyoutShowOptionsFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            FlyoutShowOptions,
            IFlyoutShowOptionsFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for FlyoutShowOptions {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFlyoutShowOptions>();
}
unsafe impl windows_core::Interface for FlyoutShowOptions {
    type Vtable = <IFlyoutShowOptions as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFlyoutShowOptions as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for FlyoutShowOptions {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.FlyoutShowOptions";
}
unsafe impl Send for FlyoutShowOptions {}
unsafe impl Sync for FlyoutShowOptions {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FocusState(pub i32);
impl FocusState {
    pub const Unfocused: Self = Self(0);
    pub const Pointer: Self = Self(1);
    pub const Keyboard: Self = Self(2);
    pub const Programmatic: Self = Self(3);
}
impl windows_core::imp::TypeKind for FocusState {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for FocusState {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Microsoft.UI.Xaml.FocusState;i4)");
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.FocusState");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontFamily(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FontFamily,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl FontFamily {
    pub fn CreateInstanceWithName(
        familyname: &windows_core::HSTRING,
    ) -> windows_core::Result<Self> {
        Self::IFontFamilyFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstanceWithName)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(familyname),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    pub fn CreateInstanceWithName_compose<T>(
        familyname: &windows_core::HSTRING,
        compose: T,
    ) -> windows_core::Result<Self>
    where
        T: windows_core::Compose,
    {
        Self::IFontFamilyFactory(|this| unsafe {
            let (derived__, base__) = windows_core::Compose::compose(compose);
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstanceWithName)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(familyname),
                core::mem::transmute_copy(&derived__),
                base__ as *mut _ as _,
                &mut result__,
            )
            .ok()?;
            let _ = &derived__;
            windows_core::imp::Type::from_abi(result__)
        })
    }
    fn IFontFamilyFactory<R, F: FnOnce(&IFontFamilyFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<FontFamily, IFontFamilyFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IFontFamilyStatics<R, F: FnOnce(&IFontFamilyStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<FontFamily, IFontFamilyStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for FontFamily {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFontFamily>();
}
unsafe impl windows_core::Interface for FontFamily {
    type Vtable = <IFontFamily as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFontFamily as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for FontFamily {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.FontFamily";
}
unsafe impl Send for FontFamily {}
unsafe impl Sync for FontFamily {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FontVariants(pub i32);
impl FontVariants {
    pub const Normal: Self = Self(0);
    pub const Superscript: Self = Self(1);
    pub const Subscript: Self = Self(2);
    pub const Ordinal: Self = Self(3);
    pub const Inferior: Self = Self(4);
    pub const Ruby: Self = Self(5);
}
impl windows_core::imp::TypeKind for FontVariants {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for FontVariants {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Microsoft.UI.Xaml.FontVariants;i4)");
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.FontVariants");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameworkElement(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FrameworkElement,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(FrameworkElement, UIElement, DependencyObject);
impl FrameworkElement {
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Resources)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetResources)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Tag)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetTag)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ActualWidth)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ActualHeight)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetWidth)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHeight)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetMaxWidth)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetMaxHeight)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetVerticalAlignment)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetMargin)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).DataContext)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetDataContext)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).RequestedTheme)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetRequestedTheme)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn IFrameworkElementFactory<
        R,
        F: FnOnce(&IFrameworkElementFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<FrameworkElement, IFrameworkElementFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IFrameworkElementStatics<
        R,
        F: FnOnce(&IFrameworkElementStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<FrameworkElement, IFrameworkElementStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for FrameworkElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFrameworkElement>();
}
unsafe impl windows_core::Interface for FrameworkElement {
    type Vtable = <IFrameworkElement as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFrameworkElement as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for FrameworkElement {
    const NAME: &'static str = "Microsoft.UI.Xaml.FrameworkElement";
}
unsafe impl Send for FrameworkElement {}
unsafe impl Sync for FrameworkElement {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameworkTemplate(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FrameworkTemplate,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(FrameworkTemplate, DependencyObject);
impl FrameworkTemplate {
    fn IFrameworkTemplateFactory<
        R,
        F: FnOnce(&IFrameworkTemplateFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            FrameworkTemplate,
            IFrameworkTemplateFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for FrameworkTemplate {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFrameworkTemplate>();
}
unsafe impl windows_core::Interface for FrameworkTemplate {
    type Vtable = <IFrameworkTemplate as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFrameworkTemplate as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for FrameworkTemplate {
    const NAME: &'static str = "Microsoft.UI.Xaml.FrameworkTemplate";
}
unsafe impl Send for FrameworkTemplate {}
unsafe impl Sync for FrameworkTemplate {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HorizontalAlignment(pub i32);
impl HorizontalAlignment {
    pub const Left: Self = Self(0);
    pub const Center: Self = Self(1);
    pub const Right: Self = Self(2);
    pub const Stretch: Self = Self(3);
}
impl windows_core::imp::TypeKind for HorizontalAlignment {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for HorizontalAlignment {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Xaml.HorizontalAlignment;i4)",
    );
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.HorizontalAlignment");
}
windows_core::imp::define_interface!(IBlock, IBlock_Vtbl, 0x8149d507_672f_5fd5_a10a_351389ba9659);
impl windows_core::RuntimeType for IBlock {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.IBlock");
}
#[repr(C)]
pub struct IBlock_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IBlockFactory,
    IBlockFactory_Vtbl,
    0x21bd671c_33e2_56ef_be37_a128e898452c
);
impl windows_core::RuntimeType for IBlockFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.IBlockFactory");
}
#[repr(C)]
pub struct IBlockFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IBlockStatics,
    IBlockStatics_Vtbl,
    0x830feedf_9aa6_56cd_983e_055500171b45
);
impl windows_core::RuntimeType for IBlockStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.IBlockStatics");
}
#[repr(C)]
pub struct IBlockStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IBorder,
    IBorder_Vtbl,
    0x1ca13b47_ff5c_5abc_a411_a177df9482a9
);
impl windows_core::RuntimeType for IBorder {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IBorder");
}
#[repr(C)]
pub struct IBorder_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    BorderBrush: usize,
    pub SetBorderBrush: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    BorderThickness: usize,
    pub SetBorderThickness:
        unsafe extern "system" fn(*mut core::ffi::c_void, Thickness) -> windows_core::HRESULT,
    Background: usize,
    pub SetBackground: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    BackgroundSizing: usize,
    SetBackgroundSizing: usize,
    CornerRadius: usize,
    pub SetCornerRadius:
        unsafe extern "system" fn(*mut core::ffi::c_void, CornerRadius) -> windows_core::HRESULT,
    Padding: usize,
    pub SetPadding:
        unsafe extern "system" fn(*mut core::ffi::c_void, Thickness) -> windows_core::HRESULT,
    Child: usize,
    pub SetChild: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IBorderStatics,
    IBorderStatics_Vtbl,
    0x9385cd66_c178_5176_b4a5_160e00e93935
);
impl windows_core::RuntimeType for IBorderStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IBorderStatics");
}
#[repr(C)]
pub struct IBorderStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IBringIntoViewOptions,
    IBringIntoViewOptions_Vtbl,
    0xeeb4a447_eb9e_5003_a479_b9e3a886b708
);
impl windows_core::RuntimeType for IBringIntoViewOptions {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IBringIntoViewOptions");
}
#[repr(C)]
pub struct IBringIntoViewOptions_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    AnimationDesired: usize,
    SetAnimationDesired: usize,
    TargetRect: usize,
    pub SetTargetRect: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    HorizontalAlignmentRatio: usize,
    SetHorizontalAlignmentRatio: usize,
    VerticalAlignmentRatio: usize,
    pub SetVerticalAlignmentRatio:
        unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(IBrush, IBrush_Vtbl, 0x2de3cb83_1329_5679_88f8_c822bc5442cb);
impl windows_core::RuntimeType for IBrush {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Media.IBrush");
}
#[repr(C)]
pub struct IBrush_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IBrushFactory,
    IBrushFactory_Vtbl,
    0xb5258717_6c49_5ba5_87fd_35df382647a5
);
impl windows_core::RuntimeType for IBrushFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Media.IBrushFactory");
}
#[repr(C)]
pub struct IBrushFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IBrushStatics,
    IBrushStatics_Vtbl,
    0x5b854f50_f818_5f01_91b0_28132d3f5957
);
impl windows_core::RuntimeType for IBrushStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Media.IBrushStatics");
}
#[repr(C)]
pub struct IBrushStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IButton,
    IButton_Vtbl,
    0x216c183d_d07a_5aa5_b8a4_0300a2683e87
);
impl windows_core::RuntimeType for IButton {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IButton");
}
#[repr(C)]
pub struct IButton_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Flyout: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetFlyout: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IButtonBase,
    IButtonBase_Vtbl,
    0x65714269_2473_5327_a652_0ea6bce7f403
);
impl windows_core::RuntimeType for IButtonBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.IButtonBase",
    );
}
#[repr(C)]
pub struct IButtonBase_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IButtonBaseFactory,
    IButtonBaseFactory_Vtbl,
    0x21251aa9_6fd1_5e51_ab3b_e6fcaf3395ed
);
impl windows_core::RuntimeType for IButtonBaseFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.IButtonBaseFactory",
    );
}
#[repr(C)]
pub struct IButtonBaseFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IButtonBaseStatics,
    IButtonBaseStatics_Vtbl,
    0xdbe812f6_adf8_51d3_8137_a8fbf6445b3c
);
impl windows_core::RuntimeType for IButtonBaseStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.IButtonBaseStatics",
    );
}
#[repr(C)]
pub struct IButtonBaseStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IButtonFactory,
    IButtonFactory_Vtbl,
    0xfe393422_d91c_57b1_9a9c_2c7e3f41f77c
);
impl windows_core::RuntimeType for IButtonFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IButtonFactory");
}
#[repr(C)]
pub struct IButtonFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IButtonStatics,
    IButtonStatics_Vtbl,
    0x57823d25_b26a_5e0f_94f6_bbae70683dc5
);
impl windows_core::RuntimeType for IButtonStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IButtonStatics");
}
#[repr(C)]
pub struct IButtonStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ICompositionBrush,
    ICompositionBrush_Vtbl,
    0xab0d7608_30c0_40e9_b568_b60a6bd1fb46
);
impl windows_core::RuntimeType for ICompositionBrush {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Windows.UI.Composition.ICompositionBrush");
}
#[repr(C)]
pub struct ICompositionBrush_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ICompositionObject,
    ICompositionObject_Vtbl,
    0xbcb4ad45_7609_4550_934f_16002a68fded
);
impl windows_core::RuntimeType for ICompositionObject {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Windows.UI.Composition.ICompositionObject");
}
#[repr(C)]
pub struct ICompositionObject_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ICompositionObjectStatics,
    ICompositionObjectStatics_Vtbl,
    0xa091356c_38ae_514c_9b58_ad5c8c7936ad
);
impl windows_core::RuntimeType for ICompositionObjectStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Composition.ICompositionObjectStatics",
    );
}
#[repr(C)]
pub struct ICompositionObjectStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ICompositionSupportsSystemBackdrop,
    ICompositionSupportsSystemBackdrop_Vtbl,
    0x397dafe4_b6c2_5bb9_951d_f5707de8b7bc
);
impl windows_core::RuntimeType for ICompositionSupportsSystemBackdrop {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Composition.ICompositionSupportsSystemBackdrop",
    );
}
windows_core::imp::interface_hierarchy!(
    ICompositionSupportsSystemBackdrop,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl ICompositionSupportsSystemBackdrop {
    pub fn SystemBackdrop(&self) -> windows_core::Result<CompositionBrush> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SystemBackdrop)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetSystemBackdrop<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<CompositionBrush>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetSystemBackdrop)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
}
impl windows_core::RuntimeName for ICompositionSupportsSystemBackdrop {
    const NAME: &'static str = "Microsoft.UI.Composition.ICompositionSupportsSystemBackdrop";
}
pub trait ICompositionSupportsSystemBackdrop_Impl: windows_core::IUnknownImpl {
    fn SystemBackdrop(&self) -> windows_core::Result<CompositionBrush>;
    fn SetSystemBackdrop(
        &self,
        value: windows_core::Ref<CompositionBrush>,
    ) -> windows_core::Result<()>;
}
impl ICompositionSupportsSystemBackdrop_Vtbl {
    pub const fn new<Identity: ICompositionSupportsSystemBackdrop_Impl, const OFFSET: isize>()
    -> Self {
        unsafe extern "system" fn SystemBackdrop<
            Identity: ICompositionSupportsSystemBackdrop_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            result__: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                match ICompositionSupportsSystemBackdrop_Impl::SystemBackdrop(this) {
                    Ok(ok__) => {
                        result__.write(core::mem::transmute_copy(&ok__));
                        core::mem::forget(ok__);
                        windows_core::HRESULT(0)
                    }
                    Err(err) => err.into(),
                }
            }
        }
        unsafe extern "system" fn SetSystemBackdrop<
            Identity: ICompositionSupportsSystemBackdrop_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            value: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                ICompositionSupportsSystemBackdrop_Impl::SetSystemBackdrop(
                    this,
                    core::mem::transmute_copy(&value),
                )
                .into()
            }
        }
        Self {
            base__: windows_core::IInspectable_Vtbl::new::<
                Identity,
                ICompositionSupportsSystemBackdrop,
                OFFSET,
            >(),
            SystemBackdrop: SystemBackdrop::<Identity, OFFSET>,
            SetSystemBackdrop: SetSystemBackdrop::<Identity, OFFSET>,
        }
    }
    pub fn matches(iid: &windows_core::GUID) -> bool {
        iid == &<ICompositionSupportsSystemBackdrop as windows_core::Interface>::IID
    }
}
#[repr(C)]
pub struct ICompositionSupportsSystemBackdrop_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub SystemBackdrop: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetSystemBackdrop: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IContentControl,
    IContentControl_Vtbl,
    0x07e81761_11b2_52ae_8f8b_4d53d2b5900a
);
impl windows_core::RuntimeType for IContentControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IContentControl");
}
#[repr(C)]
pub struct IContentControl_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Content: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetContent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    ContentTemplate: usize,
    pub SetContentTemplate: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IContentControlFactory,
    IContentControlFactory_Vtbl,
    0x3dea958e_5acd_5f80_8938_38634f51493a
);
impl windows_core::RuntimeType for IContentControlFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.IContentControlFactory",
    );
}
#[repr(C)]
pub struct IContentControlFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IContentControlStatics,
    IContentControlStatics_Vtbl,
    0xf25484f4_2fed_5a0a_8864_7d6d4ac43ef8
);
impl windows_core::RuntimeType for IContentControlStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.IContentControlStatics",
    );
}
#[repr(C)]
pub struct IContentControlStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IContentDialog,
    IContentDialog_Vtbl,
    0xac2145a3_4a32_5305_a81d_47509515bfce
);
impl windows_core::RuntimeType for IContentDialog {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IContentDialog");
}
#[repr(C)]
pub struct IContentDialog_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Title: usize,
    SetTitle: usize,
    TitleTemplate: usize,
    SetTitleTemplate: usize,
    FullSizeDesired: usize,
    SetFullSizeDesired: usize,
    PrimaryButtonText: usize,
    SetPrimaryButtonText: usize,
    SecondaryButtonText: usize,
    SetSecondaryButtonText: usize,
    CloseButtonText: usize,
    SetCloseButtonText: usize,
    PrimaryButtonCommand: usize,
    SetPrimaryButtonCommand: usize,
    SecondaryButtonCommand: usize,
    SetSecondaryButtonCommand: usize,
    CloseButtonCommand: usize,
    SetCloseButtonCommand: usize,
    PrimaryButtonCommandParameter: usize,
    SetPrimaryButtonCommandParameter: usize,
    SecondaryButtonCommandParameter: usize,
    SetSecondaryButtonCommandParameter: usize,
    CloseButtonCommandParameter: usize,
    SetCloseButtonCommandParameter: usize,
    IsPrimaryButtonEnabled: usize,
    SetIsPrimaryButtonEnabled: usize,
    IsSecondaryButtonEnabled: usize,
    SetIsSecondaryButtonEnabled: usize,
    PrimaryButtonStyle: usize,
    SetPrimaryButtonStyle: usize,
    SecondaryButtonStyle: usize,
    SetSecondaryButtonStyle: usize,
    CloseButtonStyle: usize,
    SetCloseButtonStyle: usize,
    DefaultButton: usize,
    pub SetDefaultButton: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        ContentDialogButton,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IContentDialogFactory,
    IContentDialogFactory_Vtbl,
    0xa05b3ad7_c60e_545a_9ee4_f098220ed816
);
impl windows_core::RuntimeType for IContentDialogFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.IContentDialogFactory",
    );
}
#[repr(C)]
pub struct IContentDialogFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IContentDialogStatics,
    IContentDialogStatics_Vtbl,
    0x397b18dd_6964_5056_97c3_d69cd7716a4b
);
impl windows_core::RuntimeType for IContentDialogStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.IContentDialogStatics",
    );
}
#[repr(C)]
pub struct IContentDialogStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IContentIslandEnvironment,
    IContentIslandEnvironment_Vtbl,
    0xc334236d_da88_566d_811d_74aef2eba978
);
impl windows_core::RuntimeType for IContentIslandEnvironment {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Content.IContentIslandEnvironment",
    );
}
#[repr(C)]
pub struct IContentIslandEnvironment_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub AppWindowId:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut WindowId) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IContextRequestedEventArgs,
    IContextRequestedEventArgs_Vtbl,
    0xbcedcb98_77b5_53c0_802e_fd52f3806e51
);
impl windows_core::RuntimeType for IContextRequestedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Input.IContextRequestedEventArgs",
    );
}
#[repr(C)]
pub struct IContextRequestedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Handled:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetHandled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    pub TryGetPosition: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut Point,
        *mut bool,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IControl,
    IControl_Vtbl,
    0x857d6e8a_d45a_5c69_a99c_bf6a5c54fb38
);
impl windows_core::RuntimeType for IControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IControl");
}
#[repr(C)]
pub struct IControl_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    IsFocusEngagementEnabled: usize,
    SetIsFocusEngagementEnabled: usize,
    IsFocusEngaged: usize,
    SetIsFocusEngaged: usize,
    RequiresPointer: usize,
    SetRequiresPointer: usize,
    FontSize: usize,
    SetFontSize: usize,
    FontFamily: usize,
    pub SetFontFamily: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    FontWeight: usize,
    SetFontWeight: usize,
    FontStyle: usize,
    SetFontStyle: usize,
    FontStretch: usize,
    SetFontStretch: usize,
    CharacterSpacing: usize,
    SetCharacterSpacing: usize,
    Foreground: usize,
    SetForeground: usize,
    IsTextScaleFactorEnabled: usize,
    SetIsTextScaleFactorEnabled: usize,
    IsEnabled: usize,
    SetIsEnabled: usize,
    TabNavigation: usize,
    SetTabNavigation: usize,
    Template: usize,
    SetTemplate: usize,
    Padding: usize,
    SetPadding: usize,
    HorizontalContentAlignment: usize,
    pub SetHorizontalContentAlignment: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        HorizontalAlignment,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IControlFactory,
    IControlFactory_Vtbl,
    0x25159233_9438_5534_aeb9_00eb059cf73f
);
impl windows_core::RuntimeType for IControlFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IControlFactory");
}
#[repr(C)]
pub struct IControlFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IControlStatics,
    IControlStatics_Vtbl,
    0xc3ae388d_aa36_5e10_acac_98415f47bcc7
);
impl windows_core::RuntimeType for IControlStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IControlStatics");
}
#[repr(C)]
pub struct IControlStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IDataPackage,
    IDataPackage_Vtbl,
    0x61ebf5c7_efea_4346_9554_981d7e198ffe
);
impl windows_core::RuntimeType for IDataPackage {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Windows.ApplicationModel.DataTransfer.IDataPackage",
    );
}
#[repr(C)]
pub struct IDataPackage_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    GetView: usize,
    Properties: usize,
    RequestedOperation: usize,
    pub SetRequestedOperation: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        DataPackageOperation,
    ) -> windows_core::HRESULT,
    OperationCompleted: usize,
    RemoveOperationCompleted: usize,
    Destroyed: usize,
    RemoveDestroyed: usize,
    SetData: usize,
    SetDataProvider: usize,
    pub SetText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDataTemplate,
    IDataTemplate_Vtbl,
    0x08fa70fa_ee75_5e92_a101_f52d0e1e9fab
);
impl windows_core::RuntimeType for IDataTemplate {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IDataTemplate");
}
#[repr(C)]
pub struct IDataTemplate_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IDataTemplateFactory,
    IDataTemplateFactory_Vtbl,
    0xd8e8249d_305b_5ca5_acf8_3e1beffd0219
);
impl windows_core::RuntimeType for IDataTemplateFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IDataTemplateFactory");
}
#[repr(C)]
pub struct IDataTemplateFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IDataTemplateStatics,
    IDataTemplateStatics_Vtbl,
    0xcf6ada69_4bf1_5f2d_8bdb_09ea1a26f975
);
impl windows_core::RuntimeType for IDataTemplateStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IDataTemplateStatics");
}
#[repr(C)]
pub struct IDataTemplateStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IDependencyObject,
    IDependencyObject_Vtbl,
    0xe7beaee7_160e_50f7_8789_d63463f979fa
);
impl windows_core::RuntimeType for IDependencyObject {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IDependencyObject");
}
#[repr(C)]
pub struct IDependencyObject_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IDependencyObjectFactory,
    IDependencyObjectFactory_Vtbl,
    0x936b614c_475f_5d7d_b3f7_bf1fbea28126
);
impl windows_core::RuntimeType for IDependencyObjectFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IDependencyObjectFactory");
}
#[repr(C)]
pub struct IDependencyObjectFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IDesktopAcrylicController,
    IDesktopAcrylicController_Vtbl,
    0x7c20a6af_8eb3_5f08_bdfc_6d35e35dfe45
);
impl windows_core::RuntimeType for IDesktopAcrylicController {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Composition.SystemBackdrops.IDesktopAcrylicController",
    );
}
#[repr(C)]
pub struct IDesktopAcrylicController_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    FallbackColor: usize,
    SetFallbackColor: usize,
    LuminosityOpacity: usize,
    pub SetLuminosityOpacity:
        unsafe extern "system" fn(*mut core::ffi::c_void, f32) -> windows_core::HRESULT,
    TintColor: usize,
    SetTintColor: usize,
    TintOpacity: usize,
    pub SetTintOpacity:
        unsafe extern "system" fn(*mut core::ffi::c_void, f32) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDesktopAcrylicControllerStatics,
    IDesktopAcrylicControllerStatics_Vtbl,
    0xa9e8f790_79ef_5416_9b67_6bcfe867c8b7
);
impl windows_core::RuntimeType for IDesktopAcrylicControllerStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Composition.SystemBackdrops.IDesktopAcrylicControllerStatics",
    );
}
#[repr(C)]
pub struct IDesktopAcrylicControllerStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IDoubleTappedRoutedEventArgs,
    IDoubleTappedRoutedEventArgs_Vtbl,
    0x32b9549d_11d8_53a5_a953_02409537a11f
);
impl windows_core::RuntimeType for IDoubleTappedRoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Input.IDoubleTappedRoutedEventArgs",
    );
}
#[repr(C)]
pub struct IDoubleTappedRoutedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    PointerDeviceType: usize,
    pub Handled:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetHandled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    pub GetPosition: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut Point,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDragEventArgs,
    IDragEventArgs_Vtbl,
    0x47ac5757_e4bc_52ba_8ab9_1bf81aad7900
);
impl windows_core::RuntimeType for IDragEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IDragEventArgs");
}
#[repr(C)]
pub struct IDragEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Handled:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetHandled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    Data: usize,
    SetData: usize,
    DataView: usize,
    DragUIOverride: usize,
    Modifiers: usize,
    pub AcceptedOperation: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut DataPackageOperation,
    ) -> windows_core::HRESULT,
    pub SetAcceptedOperation: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        DataPackageOperation,
    ) -> windows_core::HRESULT,
    AllowedOperations: usize,
    GetDeferral: usize,
    pub GetPosition: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut Point,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDragItemsStartingEventArgs,
    IDragItemsStartingEventArgs_Vtbl,
    0xa6938886_20df_558e_ac74_bb5b7f2f7e90
);
impl windows_core::RuntimeType for IDragItemsStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.IDragItemsStartingEventArgs",
    );
}
#[repr(C)]
pub struct IDragItemsStartingEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Cancel:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetCancel: unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    pub Items: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Data: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFlyoutBase,
    IFlyoutBase_Vtbl,
    0xbb6603bf_744d_5c31_a87d_744394634d77
);
impl windows_core::RuntimeType for IFlyoutBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.IFlyoutBase",
    );
}
#[repr(C)]
pub struct IFlyoutBase_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Placement: usize,
    SetPlacement: usize,
    Target: usize,
    AllowFocusOnInteraction: usize,
    SetAllowFocusOnInteraction: usize,
    LightDismissOverlayMode: usize,
    SetLightDismissOverlayMode: usize,
    AllowFocusWhenDisabled: usize,
    SetAllowFocusWhenDisabled: usize,
    ShowMode: usize,
    SetShowMode: usize,
    InputDevicePrefersPrimaryCommands: usize,
    AreOpenCloseAnimationsEnabled: usize,
    SetAreOpenCloseAnimationsEnabled: usize,
    ShouldConstrainToRootBounds: usize,
    SetShouldConstrainToRootBounds: usize,
    IsConstrainedToRootBounds: usize,
    ElementSoundMode: usize,
    SetElementSoundMode: usize,
    OverlayInputPassThroughElement: usize,
    SetOverlayInputPassThroughElement: usize,
    IsOpen: usize,
    XamlRoot: usize,
    SetXamlRoot: usize,
    Opened: usize,
    RemoveOpened: usize,
    Closed: usize,
    RemoveClosed: usize,
    Opening: usize,
    RemoveOpening: usize,
    Closing: usize,
    RemoveClosing: usize,
    pub ShowAt: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ShowAtWithOptions: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Hide: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFlyoutBaseFactory,
    IFlyoutBaseFactory_Vtbl,
    0x006d738f_7c91_5ef3_8a80_a548108dab8b
);
impl windows_core::RuntimeType for IFlyoutBaseFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.IFlyoutBaseFactory",
    );
}
#[repr(C)]
pub struct IFlyoutBaseFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IFlyoutBaseStatics,
    IFlyoutBaseStatics_Vtbl,
    0x63ee628e_d2e3_5515_aea4_e461088c0c4e
);
impl windows_core::RuntimeType for IFlyoutBaseStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.IFlyoutBaseStatics",
    );
}
#[repr(C)]
pub struct IFlyoutBaseStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IFlyoutBaseStatics2,
    IFlyoutBaseStatics2_Vtbl,
    0x2403cd2c_a6b8_5dc2_be3b_2a4bdd072ef1
);
impl windows_core::RuntimeType for IFlyoutBaseStatics2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.IFlyoutBaseStatics2",
    );
}
#[repr(C)]
pub struct IFlyoutBaseStatics2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IFlyoutShowOptions,
    IFlyoutShowOptions_Vtbl,
    0x30774a93_2803_50d3_b406_904aec3e175d
);
impl windows_core::RuntimeType for IFlyoutShowOptions {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.IFlyoutShowOptions",
    );
}
#[repr(C)]
pub struct IFlyoutShowOptions_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Position: usize,
    pub SetPosition: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFlyoutShowOptionsFactory,
    IFlyoutShowOptionsFactory_Vtbl,
    0x17426d30_70d9_54d7_bd39_e7c4c940c0f4
);
impl windows_core::RuntimeType for IFlyoutShowOptionsFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.IFlyoutShowOptionsFactory",
    );
}
#[repr(C)]
pub struct IFlyoutShowOptionsFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFontFamily,
    IFontFamily_Vtbl,
    0x18fa5bc1_7294_527c_bb02_b213e0b3a2a3
);
impl windows_core::RuntimeType for IFontFamily {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Media.IFontFamily");
}
#[repr(C)]
pub struct IFontFamily_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IFontFamilyFactory,
    IFontFamilyFactory_Vtbl,
    0x61b88a77_d0f9_5e9e_8c28_eda01fede22e
);
impl windows_core::RuntimeType for IFontFamilyFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Media.IFontFamilyFactory");
}
#[repr(C)]
pub struct IFontFamilyFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstanceWithName: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFontFamilyStatics,
    IFontFamilyStatics_Vtbl,
    0xb3eadceb_c471_58fe_93d0_d71b04a7fd54
);
impl windows_core::RuntimeType for IFontFamilyStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Media.IFontFamilyStatics");
}
#[repr(C)]
pub struct IFontFamilyStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IFrameworkElement,
    IFrameworkElement_Vtbl,
    0xfe08f13d_dc6a_5495_ad44_c2d8d21863b0
);
impl windows_core::RuntimeType for IFrameworkElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IFrameworkElement");
}
#[repr(C)]
pub struct IFrameworkElement_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Triggers: usize,
    pub Resources: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetResources: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Tag: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetTag: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Language: usize,
    SetLanguage: usize,
    pub ActualWidth:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    pub ActualHeight:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    Width: usize,
    pub SetWidth: unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    Height: usize,
    pub SetHeight: unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    MinWidth: usize,
    SetMinWidth: usize,
    MaxWidth: usize,
    pub SetMaxWidth:
        unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    MinHeight: usize,
    SetMinHeight: usize,
    MaxHeight: usize,
    pub SetMaxHeight:
        unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    HorizontalAlignment: usize,
    pub SetHorizontalAlignment: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        HorizontalAlignment,
    ) -> windows_core::HRESULT,
    VerticalAlignment: usize,
    pub SetVerticalAlignment: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        VerticalAlignment,
    ) -> windows_core::HRESULT,
    Margin: usize,
    pub SetMargin:
        unsafe extern "system" fn(*mut core::ffi::c_void, Thickness) -> windows_core::HRESULT,
    Name: usize,
    SetName: usize,
    BaseUri: usize,
    pub DataContext: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetDataContext: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    AllowFocusOnInteraction: usize,
    SetAllowFocusOnInteraction: usize,
    FocusVisualMargin: usize,
    SetFocusVisualMargin: usize,
    FocusVisualSecondaryThickness: usize,
    SetFocusVisualSecondaryThickness: usize,
    FocusVisualPrimaryThickness: usize,
    SetFocusVisualPrimaryThickness: usize,
    FocusVisualSecondaryBrush: usize,
    SetFocusVisualSecondaryBrush: usize,
    FocusVisualPrimaryBrush: usize,
    SetFocusVisualPrimaryBrush: usize,
    AllowFocusWhenDisabled: usize,
    SetAllowFocusWhenDisabled: usize,
    Style: usize,
    SetStyle: usize,
    Parent: usize,
    FlowDirection: usize,
    SetFlowDirection: usize,
    pub RequestedTheme: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut ElementTheme,
    ) -> windows_core::HRESULT,
    pub SetRequestedTheme:
        unsafe extern "system" fn(*mut core::ffi::c_void, ElementTheme) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFrameworkElementFactory,
    IFrameworkElementFactory_Vtbl,
    0xbd3f2272_3efa_5f92_b759_90b1cc3e784c
);
impl windows_core::RuntimeType for IFrameworkElementFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IFrameworkElementFactory");
}
#[repr(C)]
pub struct IFrameworkElementFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IFrameworkElementStatics,
    IFrameworkElementStatics_Vtbl,
    0x894e2704_14e7_569a_b21e_afc7df7145a1
);
impl windows_core::RuntimeType for IFrameworkElementStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IFrameworkElementStatics");
}
#[repr(C)]
pub struct IFrameworkElementStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IFrameworkTemplate,
    IFrameworkTemplate_Vtbl,
    0x0084c7c2_de48_5b0b_8a5a_e4fb76b7f7d1
);
impl windows_core::RuntimeType for IFrameworkTemplate {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IFrameworkTemplate");
}
#[repr(C)]
pub struct IFrameworkTemplate_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IFrameworkTemplateFactory,
    IFrameworkTemplateFactory_Vtbl,
    0x616dd6db_b064_561d_b162_46ceb45dc562
);
impl windows_core::RuntimeType for IFrameworkTemplateFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IFrameworkTemplateFactory");
}
#[repr(C)]
pub struct IFrameworkTemplateFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInline,
    IInline_Vtbl,
    0x813d427a_8980_5a79_a8fa_f27919cfb24f
);
impl windows_core::RuntimeType for IInline {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.IInline");
}
#[repr(C)]
pub struct IInline_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInlineFactory,
    IInlineFactory_Vtbl,
    0xfd253a36_fa2b_5b30_89a8_9f577871ec07
);
impl windows_core::RuntimeType for IInlineFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.IInlineFactory");
}
#[repr(C)]
pub struct IInlineFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInlineUIContainer,
    IInlineUIContainer_Vtbl,
    0xd529bef6_c05a_5bad_85e8_640127cf86f5
);
impl windows_core::RuntimeType for IInlineUIContainer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Documents.IInlineUIContainer",
    );
}
#[repr(C)]
pub struct IInlineUIContainer_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Child: usize,
    pub SetChild: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IInputCursor,
    IInputCursor_Vtbl,
    0x359b15f9_19c2_5714_8432_75176826406b
);
impl windows_core::RuntimeType for IInputCursor {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Input.IInputCursor");
}
#[repr(C)]
pub struct IInputCursor_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInputCursorFactory,
    IInputCursorFactory_Vtbl,
    0x2f47647b_4be0_53e9_be7e_c38d5459db6b
);
impl windows_core::RuntimeType for IInputCursorFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Input.IInputCursorFactory");
}
#[repr(C)]
pub struct IInputCursorFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInputCursorStatics,
    IInputCursorStatics_Vtbl,
    0x92f6a552_099f_55fb_8c31_e450284c9643
);
impl windows_core::RuntimeType for IInputCursorStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Input.IInputCursorStatics");
}
#[repr(C)]
pub struct IInputCursorStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInputKeyboardSource,
    IInputKeyboardSource_Vtbl,
    0xed61b906_16ad_5df7_a550_5e6f7d2229f7
);
impl windows_core::RuntimeType for IInputKeyboardSource {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Input.IInputKeyboardSource");
}
#[repr(C)]
pub struct IInputKeyboardSource_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInputKeyboardSourceStatics,
    IInputKeyboardSourceStatics_Vtbl,
    0xf4e1563d_8c2e_5bcd_b784_47adeaa3cd7e
);
impl windows_core::RuntimeType for IInputKeyboardSourceStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Input.IInputKeyboardSourceStatics",
    );
}
#[repr(C)]
pub struct IInputKeyboardSourceStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub GetKeyStateForCurrentThread: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        VirtualKey,
        *mut CoreVirtualKeyStates,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IInputKeyboardSourceStatics2,
    IInputKeyboardSourceStatics2_Vtbl,
    0x8857518c_2899_5f11_9b64_0ad83234824b
);
impl windows_core::RuntimeType for IInputKeyboardSourceStatics2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Input.IInputKeyboardSourceStatics2",
    );
}
#[repr(C)]
pub struct IInputKeyboardSourceStatics2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInputKeyboardSourceStatics3,
    IInputKeyboardSourceStatics3_Vtbl,
    0x34b960c5_a5ae_52af_8566_6d2d55ff52d1
);
impl windows_core::RuntimeType for IInputKeyboardSourceStatics3 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Input.IInputKeyboardSourceStatics3",
    );
}
#[repr(C)]
pub struct IInputKeyboardSourceStatics3_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInputObject,
    IInputObject_Vtbl,
    0x42edbc88_d386_544d_b1b8_68617fe68282
);
impl windows_core::RuntimeType for IInputObject {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Input.IInputObject");
}
#[repr(C)]
pub struct IInputObject_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInputSystemCursor,
    IInputSystemCursor_Vtbl,
    0x59f538e7_c500_59ab_8b54_0bc6100fd49e
);
impl windows_core::RuntimeType for IInputSystemCursor {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Input.IInputSystemCursor");
}
#[repr(C)]
pub struct IInputSystemCursor_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInputSystemCursorStatics,
    IInputSystemCursorStatics_Vtbl,
    0xd3860bb6_698a_5814_aedd_c2fa8bba5a02
);
impl windows_core::RuntimeType for IInputSystemCursorStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Input.IInputSystemCursorStatics");
}
#[repr(C)]
pub struct IInputSystemCursorStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Create: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        InputSystemCursorShape,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IItemsControl,
    IItemsControl_Vtbl,
    0xbf1ccb54_83e2_5b98_acbc_736f876c3d35
);
impl windows_core::RuntimeType for IItemsControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IItemsControl");
}
#[repr(C)]
pub struct IItemsControl_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IItemsControlFactory,
    IItemsControlFactory_Vtbl,
    0xb2686273_2dd3_5454_9f25_247e66270336
);
impl windows_core::RuntimeType for IItemsControlFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.IItemsControlFactory",
    );
}
#[repr(C)]
pub struct IItemsControlFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IItemsControlStatics,
    IItemsControlStatics_Vtbl,
    0x4fb961e1_5f91_5585_a692_6825cde3ed1e
);
impl windows_core::RuntimeType for IItemsControlStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.IItemsControlStatics",
    );
}
#[repr(C)]
pub struct IItemsControlStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IKeyRoutedEventArgs,
    IKeyRoutedEventArgs_Vtbl,
    0xee357007_a2d6_5c75_9431_05fd66ec7915
);
impl windows_core::RuntimeType for IKeyRoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Input.IKeyRoutedEventArgs");
}
#[repr(C)]
pub struct IKeyRoutedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Key:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut VirtualKey) -> windows_core::HRESULT,
    KeyStatus: usize,
    pub Handled:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetHandled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    pub OriginalKey:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut VirtualKey) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ILineBreak,
    ILineBreak_Vtbl,
    0x09307599_7cc2_5f54_b106_728620c16f76
);
impl windows_core::RuntimeType for ILineBreak {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.ILineBreak");
}
#[repr(C)]
pub struct ILineBreak_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IListViewBase,
    IListViewBase_Vtbl,
    0x775c57ac_abce_5beb_8e34_3b8158aedd80
);
impl windows_core::RuntimeType for IListViewBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IListViewBase");
}
#[repr(C)]
pub struct IListViewBase_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub SelectedItems: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    SelectionMode: usize,
    SetSelectionMode: usize,
    IsSwipeEnabled: usize,
    SetIsSwipeEnabled: usize,
    CanDragItems: usize,
    SetCanDragItems: usize,
    CanReorderItems: usize,
    SetCanReorderItems: usize,
    IsItemClickEnabled: usize,
    SetIsItemClickEnabled: usize,
    DataFetchSize: usize,
    SetDataFetchSize: usize,
    IncrementalLoadingThreshold: usize,
    SetIncrementalLoadingThreshold: usize,
    IncrementalLoadingTrigger: usize,
    SetIncrementalLoadingTrigger: usize,
    ShowsScrollingPlaceholders: usize,
    SetShowsScrollingPlaceholders: usize,
    ReorderMode: usize,
    SetReorderMode: usize,
    SelectedRanges: usize,
    IsMultiSelectCheckBoxEnabled: usize,
    SetIsMultiSelectCheckBoxEnabled: usize,
    SingleSelectionFollowsFocus: usize,
    SetSingleSelectionFollowsFocus: usize,
    ItemClick: usize,
    RemoveItemClick: usize,
    pub DragItemsStarting: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveDragItemsStarting:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IListViewBaseFactory,
    IListViewBaseFactory_Vtbl,
    0x44092863_c09e_551e_85bf_951b552950c8
);
impl windows_core::RuntimeType for IListViewBaseFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.IListViewBaseFactory",
    );
}
#[repr(C)]
pub struct IListViewBaseFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IListViewBaseStatics,
    IListViewBaseStatics_Vtbl,
    0xb8487e94_010f_56d4_9f76_94bf8c462745
);
impl windows_core::RuntimeType for IListViewBaseStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.IListViewBaseStatics",
    );
}
#[repr(C)]
pub struct IListViewBaseStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IListViewItem,
    IListViewItem_Vtbl,
    0x05fe41c2_0451_5d38_9c55_5d10cfd08889
);
impl windows_core::RuntimeType for IListViewItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IListViewItem");
}
#[repr(C)]
pub struct IListViewItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IListViewItemFactory,
    IListViewItemFactory_Vtbl,
    0xd9f4d0b8_ee59_5036_bd7a_7c89cf0bc2ac
);
impl windows_core::RuntimeType for IListViewItemFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.IListViewItemFactory",
    );
}
#[repr(C)]
pub struct IListViewItemFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IParagraph,
    IParagraph_Vtbl,
    0x9ed64c77_329d_502f_a257_f58398edab51
);
impl windows_core::RuntimeType for IParagraph {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.IParagraph");
}
#[repr(C)]
pub struct IParagraph_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Inlines: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IParagraphStatics,
    IParagraphStatics_Vtbl,
    0x4eb89ab1_66c8_5fc0_aa5f_48c8092ceb5f
);
impl windows_core::RuntimeType for IParagraphStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Documents.IParagraphStatics",
    );
}
#[repr(C)]
pub struct IParagraphStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IPointer,
    IPointer_Vtbl,
    0x1f9afbf5_11a3_5e68_aa1b_72febfa0ab23
);
impl windows_core::RuntimeType for IPointer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Input.IPointer");
}
#[repr(C)]
pub struct IPointer_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IPointerPoint,
    IPointerPoint_Vtbl,
    0x0d430ee6_252c_59a4_b2a2_d44264dc6a40
);
impl windows_core::RuntimeType for IPointerPoint {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Input.IPointerPoint");
}
#[repr(C)]
pub struct IPointerPoint_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    FrameId: usize,
    IsInContact: usize,
    PointerDeviceType: usize,
    PointerId: usize,
    pub Position:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut Point) -> windows_core::HRESULT,
    pub Properties: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Timestamp:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IPointerPointProperties,
    IPointerPointProperties_Vtbl,
    0xd760ed77_4b10_57a5_b3cc_d9bf3413e996
);
impl windows_core::RuntimeType for IPointerPointProperties {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Input.IPointerPointProperties");
}
#[repr(C)]
pub struct IPointerPointProperties_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    ContactRect: usize,
    IsBarrelButtonPressed: usize,
    IsCanceled: usize,
    IsEraser: usize,
    pub IsHorizontalMouseWheel:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    IsInRange: usize,
    IsInverted: usize,
    pub IsLeftButtonPressed:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub IsMiddleButtonPressed:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    IsPrimary: usize,
    pub IsRightButtonPressed:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    IsXButton1Pressed: usize,
    IsXButton2Pressed: usize,
    pub MouseWheelDelta:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IPointerPointStatics,
    IPointerPointStatics_Vtbl,
    0x880d34c2_acfd_51d9_b60a_b408ce17590c
);
impl windows_core::RuntimeType for IPointerPointStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Input.IPointerPointStatics");
}
#[repr(C)]
pub struct IPointerPointStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IPointerRoutedEventArgs,
    IPointerRoutedEventArgs_Vtbl,
    0x66e78a9a_1bec_5f92_b1a1_ea6334ee511c
);
impl windows_core::RuntimeType for IPointerRoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Input.IPointerRoutedEventArgs",
    );
}
#[repr(C)]
pub struct IPointerRoutedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Pointer: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub KeyModifiers: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut VirtualKeyModifiers,
    ) -> windows_core::HRESULT,
    pub Handled:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetHandled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    IsGenerated: usize,
    pub GetCurrentPoint: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IResourceDictionary,
    IResourceDictionary_Vtbl,
    0x1b690975_a710_5783_a6e1_15836f6186c2
);
impl windows_core::RuntimeType for IResourceDictionary {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IResourceDictionary");
}
#[repr(C)]
pub struct IResourceDictionary_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IResourceDictionaryFactory,
    IResourceDictionaryFactory_Vtbl,
    0xea22a48f_ab71_56f6_a392_d82310c8aa7b
);
impl windows_core::RuntimeType for IResourceDictionaryFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IResourceDictionaryFactory");
}
#[repr(C)]
pub struct IResourceDictionaryFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IRichTextBlock,
    IRichTextBlock_Vtbl,
    0xd766e4db_a684_50b7_a202_c8e91fa26ff3
);
impl windows_core::RuntimeType for IRichTextBlock {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IRichTextBlock");
}
#[repr(C)]
pub struct IRichTextBlock_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    FontSize: usize,
    SetFontSize: usize,
    FontFamily: usize,
    SetFontFamily: usize,
    FontWeight: usize,
    SetFontWeight: usize,
    FontStyle: usize,
    SetFontStyle: usize,
    FontStretch: usize,
    SetFontStretch: usize,
    Foreground: usize,
    SetForeground: usize,
    TextWrapping: usize,
    SetTextWrapping: usize,
    TextTrimming: usize,
    SetTextTrimming: usize,
    TextAlignment: usize,
    SetTextAlignment: usize,
    pub Blocks: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Padding: usize,
    SetPadding: usize,
    LineHeight: usize,
    SetLineHeight: usize,
    LineStackingStrategy: usize,
    SetLineStackingStrategy: usize,
    CharacterSpacing: usize,
    SetCharacterSpacing: usize,
    OverflowContentTarget: usize,
    SetOverflowContentTarget: usize,
    IsTextSelectionEnabled: usize,
    SetIsTextSelectionEnabled: usize,
    HasOverflowContent: usize,
    SelectedText: usize,
    ContentStart: usize,
    ContentEnd: usize,
    SelectionStart: usize,
    SelectionEnd: usize,
    BaselineOffset: usize,
    MaxLines: usize,
    SetMaxLines: usize,
    TextLineBounds: usize,
    SetTextLineBounds: usize,
    SelectionHighlightColor: usize,
    SetSelectionHighlightColor: usize,
    OpticalMarginAlignment: usize,
    SetOpticalMarginAlignment: usize,
    IsColorFontEnabled: usize,
    SetIsColorFontEnabled: usize,
    TextReadingOrder: usize,
    SetTextReadingOrder: usize,
    IsTextScaleFactorEnabled: usize,
    SetIsTextScaleFactorEnabled: usize,
    TextDecorations: usize,
    SetTextDecorations: usize,
    IsTextTrimmed: usize,
    HorizontalTextAlignment: usize,
    SetHorizontalTextAlignment: usize,
    pub TextHighlighters: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IRichTextBlockStatics,
    IRichTextBlockStatics_Vtbl,
    0x65dca245_b4fc_5718_b628_a4953f61d6a3
);
impl windows_core::RuntimeType for IRichTextBlockStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.IRichTextBlockStatics",
    );
}
#[repr(C)]
pub struct IRichTextBlockStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IRoutedEventArgs,
    IRoutedEventArgs_Vtbl,
    0x0908c407_1c7d_5de3_9c50_d971c62ec8ec
);
impl windows_core::RuntimeType for IRoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IRoutedEventArgs");
}
#[repr(C)]
pub struct IRoutedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub OriginalSource: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IRoutedEventArgsFactory,
    IRoutedEventArgsFactory_Vtbl,
    0x914b02c7_076b_5b89_98e7_6c373379e9af
);
impl windows_core::RuntimeType for IRoutedEventArgsFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IRoutedEventArgsFactory");
}
#[repr(C)]
pub struct IRoutedEventArgsFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(IRun, IRun_Vtbl, 0x1f905239_37cb_590b_9132_3ffb7741906e);
impl windows_core::RuntimeType for IRun {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.IRun");
}
#[repr(C)]
pub struct IRun_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Text: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IRunStatics,
    IRunStatics_Vtbl,
    0x051b3c5b_7600_51a5_80c5_93eb50fd684f
);
impl windows_core::RuntimeType for IRunStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.IRunStatics");
}
#[repr(C)]
pub struct IRunStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IScrollViewer,
    IScrollViewer_Vtbl,
    0x1dc28c2e_996c_5394_89c3_4dc656b4ad46
);
impl windows_core::RuntimeType for IScrollViewer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.IScrollViewer");
}
#[repr(C)]
pub struct IScrollViewer_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    HorizontalScrollBarVisibility: usize,
    SetHorizontalScrollBarVisibility: usize,
    VerticalScrollBarVisibility: usize,
    SetVerticalScrollBarVisibility: usize,
    IsHorizontalRailEnabled: usize,
    SetIsHorizontalRailEnabled: usize,
    IsVerticalRailEnabled: usize,
    SetIsVerticalRailEnabled: usize,
    IsHorizontalScrollChainingEnabled: usize,
    SetIsHorizontalScrollChainingEnabled: usize,
    IsVerticalScrollChainingEnabled: usize,
    SetIsVerticalScrollChainingEnabled: usize,
    IsZoomChainingEnabled: usize,
    SetIsZoomChainingEnabled: usize,
    IsScrollInertiaEnabled: usize,
    SetIsScrollInertiaEnabled: usize,
    IsZoomInertiaEnabled: usize,
    SetIsZoomInertiaEnabled: usize,
    HorizontalScrollMode: usize,
    SetHorizontalScrollMode: usize,
    VerticalScrollMode: usize,
    SetVerticalScrollMode: usize,
    ZoomMode: usize,
    SetZoomMode: usize,
    HorizontalSnapPointsAlignment: usize,
    SetHorizontalSnapPointsAlignment: usize,
    VerticalSnapPointsAlignment: usize,
    SetVerticalSnapPointsAlignment: usize,
    HorizontalSnapPointsType: usize,
    SetHorizontalSnapPointsType: usize,
    VerticalSnapPointsType: usize,
    SetVerticalSnapPointsType: usize,
    ZoomSnapPointsType: usize,
    SetZoomSnapPointsType: usize,
    HorizontalOffset: usize,
    ViewportWidth: usize,
    ScrollableWidth: usize,
    ComputedHorizontalScrollBarVisibility: usize,
    ExtentWidth: usize,
    VerticalOffset: usize,
    pub ViewportHeight:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    ScrollableHeight: usize,
    ComputedVerticalScrollBarVisibility: usize,
    pub ExtentHeight:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    MinZoomFactor: usize,
    SetMinZoomFactor: usize,
    MaxZoomFactor: usize,
    SetMaxZoomFactor: usize,
    ZoomFactor: usize,
    ZoomSnapPoints: usize,
    TopLeftHeader: usize,
    SetTopLeftHeader: usize,
    LeftHeader: usize,
    SetLeftHeader: usize,
    TopHeader: usize,
    SetTopHeader: usize,
    ReduceViewportForCoreInputViewOcclusions: usize,
    SetReduceViewportForCoreInputViewOcclusions: usize,
    HorizontalAnchorRatio: usize,
    SetHorizontalAnchorRatio: usize,
    VerticalAnchorRatio: usize,
    SetVerticalAnchorRatio: usize,
    CanContentRenderOutsideBounds: usize,
    SetCanContentRenderOutsideBounds: usize,
    AnchorRequested: usize,
    RemoveAnchorRequested: usize,
    ViewChanging: usize,
    RemoveViewChanging: usize,
    ViewChanged: usize,
    RemoveViewChanged: usize,
    DirectManipulationStarted: usize,
    RemoveDirectManipulationStarted: usize,
    DirectManipulationCompleted: usize,
    RemoveDirectManipulationCompleted: usize,
    ScrollToHorizontalOffset: usize,
    pub ScrollToVerticalOffset:
        unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IScrollViewerStatics,
    IScrollViewerStatics_Vtbl,
    0xd971fd86_0a96_50c4_a6e1_9975faa2a142
);
impl windows_core::RuntimeType for IScrollViewerStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.IScrollViewerStatics",
    );
}
#[repr(C)]
pub struct IScrollViewerStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ISelector,
    ISelector_Vtbl,
    0x8f7e2159_e61d_576f_8476_f83fde3d689e
);
impl windows_core::RuntimeType for ISelector {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.ISelector",
    );
}
#[repr(C)]
pub struct ISelector_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ISelectorFactory,
    ISelectorFactory_Vtbl,
    0x21a42024_af07_58f9_8789_848d3324d901
);
impl windows_core::RuntimeType for ISelectorFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.ISelectorFactory",
    );
}
#[repr(C)]
pub struct ISelectorFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ISelectorItem,
    ISelectorItem_Vtbl,
    0x5772c4de_60ea_5492_8c5e_b3323d5a3ca6
);
impl windows_core::RuntimeType for ISelectorItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.ISelectorItem",
    );
}
#[repr(C)]
pub struct ISelectorItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ISelectorItemFactory,
    ISelectorItemFactory_Vtbl,
    0x078039f5_76ed_5299_9715_fc8c58173560
);
impl windows_core::RuntimeType for ISelectorItemFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.ISelectorItemFactory",
    );
}
#[repr(C)]
pub struct ISelectorItemFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ISelectorItemStatics,
    ISelectorItemStatics_Vtbl,
    0x4b201a54_a414_5e79_9b6b_3da9de442a35
);
impl windows_core::RuntimeType for ISelectorItemStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.ISelectorItemStatics",
    );
}
#[repr(C)]
pub struct ISelectorItemStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ISelectorStatics,
    ISelectorStatics_Vtbl,
    0x569b2234_1ceb_516e_b64e_0d479452e279
);
impl windows_core::RuntimeType for ISelectorStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.Primitives.ISelectorStatics",
    );
}
#[repr(C)]
pub struct ISelectorStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ISolidColorBrush,
    ISolidColorBrush_Vtbl,
    0xb3865c31_37c8_55c1_8a72_d41c67642e2a
);
impl windows_core::RuntimeType for ISolidColorBrush {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Media.ISolidColorBrush");
}
#[repr(C)]
pub struct ISolidColorBrush_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Color:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut Color) -> windows_core::HRESULT,
    pub SetColor: unsafe extern "system" fn(*mut core::ffi::c_void, Color) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ISolidColorBrushFactory,
    ISolidColorBrushFactory_Vtbl,
    0x7b559384_4daa_54f4_91ef_33a23fd816ca
);
impl windows_core::RuntimeType for ISolidColorBrushFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Media.ISolidColorBrushFactory",
    );
}
#[repr(C)]
pub struct ISolidColorBrushFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstanceWithColor: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        Color,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ISolidColorBrushStatics,
    ISolidColorBrushStatics_Vtbl,
    0x6bc16da0_c4e6_59b8_995b_b31e48424c07
);
impl windows_core::RuntimeType for ISolidColorBrushStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Media.ISolidColorBrushStatics",
    );
}
#[repr(C)]
pub struct ISolidColorBrushStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(ISpan, ISpan_Vtbl, 0x91b93d4d_4e28_57b9_bffb_3566c2a3c2a1);
impl windows_core::RuntimeType for ISpan {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.ISpan");
}
#[repr(C)]
pub struct ISpan_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Inlines: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetInlines: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ISpanFactory,
    ISpanFactory_Vtbl,
    0xa6e87c16_c175_55c8_bbd3_ce40f9d0a680
);
impl windows_core::RuntimeType for ISpanFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.ISpanFactory");
}
#[repr(C)]
pub struct ISpanFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ISystemBackdropConfiguration,
    ISystemBackdropConfiguration_Vtbl,
    0xebcce1b9_0e0c_5431_ab0e_00f3f0669962
);
impl windows_core::RuntimeType for ISystemBackdropConfiguration {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Composition.SystemBackdrops.ISystemBackdropConfiguration",
    );
}
#[repr(C)]
pub struct ISystemBackdropConfiguration_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    HighContrastBackgroundColor: usize,
    SetHighContrastBackgroundColor: usize,
    IsHighContrast: usize,
    SetIsHighContrast: usize,
    IsInputActive: usize,
    pub SetIsInputActive:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    Theme: usize,
    pub SetTheme: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        SystemBackdropTheme,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ISystemBackdropControllerWithTargets,
    ISystemBackdropControllerWithTargets_Vtbl,
    0x9c56fe7c_98eb_5f89_ad97_dad57fc30c8c
);
impl windows_core::RuntimeType for ISystemBackdropControllerWithTargets {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Composition.SystemBackdrops.ISystemBackdropControllerWithTargets",
    );
}
windows_core::imp::interface_hierarchy!(
    ISystemBackdropControllerWithTargets,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl ISystemBackdropControllerWithTargets {
    pub fn AddSystemBackdropTarget<P0>(
        &self,
        systembackdroptarget: P0,
    ) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<ICompositionSupportsSystemBackdrop>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).AddSystemBackdropTarget)(
                windows_core::Interface::as_raw(self),
                systembackdroptarget.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn RemoveAllSystemBackdropTargets(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).RemoveAllSystemBackdropTargets)(
                windows_core::Interface::as_raw(self),
            )
            .ok()
        }
    }
    pub fn SetSystemBackdropConfiguration<P0>(&self, configuration: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<SystemBackdropConfiguration>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetSystemBackdropConfiguration)(
                windows_core::Interface::as_raw(self),
                configuration.param().abi(),
            )
            .ok()
        }
    }
}
impl windows_core::RuntimeName for ISystemBackdropControllerWithTargets {
    const NAME: &'static str =
        "Microsoft.UI.Composition.SystemBackdrops.ISystemBackdropControllerWithTargets";
}
#[repr(C)]
pub struct ISystemBackdropControllerWithTargets_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    State: usize,
    pub AddSystemBackdropTarget: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut bool,
    ) -> windows_core::HRESULT,
    pub RemoveAllSystemBackdropTargets:
        unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    RemoveSystemBackdropTarget: usize,
    pub SetSystemBackdropConfiguration: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITabView,
    ITabView_Vtbl,
    0x07b509e1_1d38_551b_95f4_4732b049f6a6
);
impl windows_core::RuntimeType for ITabView {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITabView");
}
#[repr(C)]
pub struct ITabView_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    TabWidthMode: usize,
    SetTabWidthMode: usize,
    CloseButtonOverlayMode: usize,
    SetCloseButtonOverlayMode: usize,
    TabStripHeader: usize,
    SetTabStripHeader: usize,
    TabStripHeaderTemplate: usize,
    SetTabStripHeaderTemplate: usize,
    TabStripFooter: usize,
    SetTabStripFooter: usize,
    TabStripFooterTemplate: usize,
    SetTabStripFooterTemplate: usize,
    IsAddTabButtonVisible: usize,
    SetIsAddTabButtonVisible: usize,
    AddTabButtonCommand: usize,
    SetAddTabButtonCommand: usize,
    AddTabButtonCommandParameter: usize,
    SetAddTabButtonCommandParameter: usize,
    TabCloseRequested: usize,
    RemoveTabCloseRequested: usize,
    pub TabDroppedOutside: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveTabDroppedOutside:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    AddTabButtonClick: usize,
    RemoveAddTabButtonClick: usize,
    TabItemsChanged: usize,
    RemoveTabItemsChanged: usize,
    TabItemsSource: usize,
    SetTabItemsSource: usize,
    TabItems: usize,
    TabItemTemplate: usize,
    SetTabItemTemplate: usize,
    TabItemTemplateSelector: usize,
    SetTabItemTemplateSelector: usize,
    CanDragTabs: usize,
    pub SetCanDragTabs:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    CanReorderTabs: usize,
    SetCanReorderTabs: usize,
    AllowDropTabs: usize,
    pub SetAllowDropTabs:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    SelectedIndex: usize,
    SetSelectedIndex: usize,
    SelectedItem: usize,
    SetSelectedItem: usize,
    ContainerFromItem: usize,
    ContainerFromIndex: usize,
    SelectionChanged: usize,
    RemoveSelectionChanged: usize,
    pub TabDragStarting: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveTabDragStarting:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub TabDragCompleted: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveTabDragCompleted:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub TabStripDragOver: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveTabStripDragOver:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub TabStripDrop: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveTabStripDrop:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITabViewFactory,
    ITabViewFactory_Vtbl,
    0xe7e83685_eedf_5106_9429_884435ab166b
);
impl windows_core::RuntimeType for ITabViewFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITabViewFactory");
}
#[repr(C)]
pub struct ITabViewFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITabViewItem,
    ITabViewItem_Vtbl,
    0x64980afa_97af_5190_90b3_4ba277b1113d
);
impl windows_core::RuntimeType for ITabViewItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITabViewItem");
}
#[repr(C)]
pub struct ITabViewItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITabViewItemFactory,
    ITabViewItemFactory_Vtbl,
    0xb64c2423_7e56_5d41_8a84_1ee28f9826a4
);
impl windows_core::RuntimeType for ITabViewItemFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.ITabViewItemFactory",
    );
}
#[repr(C)]
pub struct ITabViewItemFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITabViewItemStatics,
    ITabViewItemStatics_Vtbl,
    0x4cfc7eab_2eff_5457_8251_c19d374ab34d
);
impl windows_core::RuntimeType for ITabViewItemStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.ITabViewItemStatics",
    );
}
#[repr(C)]
pub struct ITabViewItemStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITabViewStatics,
    ITabViewStatics_Vtbl,
    0xf4326c08_1b94_53d7_934f_e6ae0d3d18ab
);
impl windows_core::RuntimeType for ITabViewStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITabViewStatics");
}
#[repr(C)]
pub struct ITabViewStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITabViewStatics2,
    ITabViewStatics2_Vtbl,
    0xb589da39_25f2_517c_82d1_c51d8085550e
);
impl windows_core::RuntimeType for ITabViewStatics2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITabViewStatics2");
}
#[repr(C)]
pub struct ITabViewStatics2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITabViewTabDragCompletedEventArgs,
    ITabViewTabDragCompletedEventArgs_Vtbl,
    0x791fc623_c8f6_5102_81bd_1869cde82284
);
impl windows_core::RuntimeType for ITabViewTabDragCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.ITabViewTabDragCompletedEventArgs",
    );
}
#[repr(C)]
pub struct ITabViewTabDragCompletedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITabViewTabDragStartingEventArgs,
    ITabViewTabDragStartingEventArgs_Vtbl,
    0x97682812_1a7b_53fd_8b4e_c2f70d2ad250
);
impl windows_core::RuntimeType for ITabViewTabDragStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.ITabViewTabDragStartingEventArgs",
    );
}
#[repr(C)]
pub struct ITabViewTabDragStartingEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Cancel: usize,
    SetCancel: usize,
    Data: usize,
    Item: usize,
    pub Tab: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITabViewTabDroppedOutsideEventArgs,
    ITabViewTabDroppedOutsideEventArgs_Vtbl,
    0x1f1f4d5d_0fb1_51ab_b66f_f7a322bf2d13
);
impl windows_core::RuntimeType for ITabViewTabDroppedOutsideEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.ITabViewTabDroppedOutsideEventArgs",
    );
}
#[repr(C)]
pub struct ITabViewTabDroppedOutsideEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Item: usize,
    pub Tab: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITextBlock,
    ITextBlock_Vtbl,
    0x1ac8d84f_392c_5c7e_83f5_a53e3bf0abb0
);
impl windows_core::RuntimeType for ITextBlock {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITextBlock");
}
#[repr(C)]
pub struct ITextBlock_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    FontSize: usize,
    pub SetFontSize:
        unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    FontFamily: usize,
    pub SetFontFamily: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    FontWeight: usize,
    SetFontWeight: usize,
    FontStyle: usize,
    SetFontStyle: usize,
    FontStretch: usize,
    SetFontStretch: usize,
    CharacterSpacing: usize,
    SetCharacterSpacing: usize,
    Foreground: usize,
    SetForeground: usize,
    TextWrapping: usize,
    SetTextWrapping: usize,
    TextTrimming: usize,
    SetTextTrimming: usize,
    TextAlignment: usize,
    SetTextAlignment: usize,
    pub Text: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Inlines: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Padding: usize,
    SetPadding: usize,
    LineHeight: usize,
    SetLineHeight: usize,
    LineStackingStrategy: usize,
    SetLineStackingStrategy: usize,
    IsTextSelectionEnabled: usize,
    SetIsTextSelectionEnabled: usize,
    SelectedText: usize,
    pub ContentStart: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    ContentEnd: usize,
    SelectionStart: usize,
    SelectionEnd: usize,
    BaselineOffset: usize,
    SelectionHighlightColor: usize,
    SetSelectionHighlightColor: usize,
    MaxLines: usize,
    SetMaxLines: usize,
    TextLineBounds: usize,
    SetTextLineBounds: usize,
    OpticalMarginAlignment: usize,
    SetOpticalMarginAlignment: usize,
    IsColorFontEnabled: usize,
    SetIsColorFontEnabled: usize,
    TextReadingOrder: usize,
    SetTextReadingOrder: usize,
    IsTextScaleFactorEnabled: usize,
    SetIsTextScaleFactorEnabled: usize,
    TextDecorations: usize,
    SetTextDecorations: usize,
    IsTextTrimmed: usize,
    HorizontalTextAlignment: usize,
    SetHorizontalTextAlignment: usize,
    pub TextHighlighters: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITextBlockStatics,
    ITextBlockStatics_Vtbl,
    0x3187104b_65c2_5e53_b889_c8272b1314cd
);
impl windows_core::RuntimeType for ITextBlockStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITextBlockStatics");
}
#[repr(C)]
pub struct ITextBlockStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITextBox,
    ITextBox_Vtbl,
    0x873af7c2_ab89_5d76_8dbe_3d6325669df5
);
impl windows_core::RuntimeType for ITextBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITextBox");
}
#[repr(C)]
pub struct ITextBox_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Text: usize,
    SetText: usize,
    SelectedText: usize,
    SetSelectedText: usize,
    SelectionLength: usize,
    SetSelectionLength: usize,
    SelectionStart: usize,
    SetSelectionStart: usize,
    MaxLength: usize,
    SetMaxLength: usize,
    IsReadOnly: usize,
    SetIsReadOnly: usize,
    AcceptsReturn: usize,
    SetAcceptsReturn: usize,
    TextAlignment: usize,
    SetTextAlignment: usize,
    TextWrapping: usize,
    SetTextWrapping: usize,
    IsSpellCheckEnabled: usize,
    SetIsSpellCheckEnabled: usize,
    IsTextPredictionEnabled: usize,
    SetIsTextPredictionEnabled: usize,
    InputScope: usize,
    SetInputScope: usize,
    Header: usize,
    SetHeader: usize,
    HeaderTemplate: usize,
    SetHeaderTemplate: usize,
    PlaceholderText: usize,
    SetPlaceholderText: usize,
    SelectionHighlightColor: usize,
    SetSelectionHighlightColor: usize,
    PreventKeyboardDisplayOnProgrammaticFocus: usize,
    SetPreventKeyboardDisplayOnProgrammaticFocus: usize,
    IsColorFontEnabled: usize,
    SetIsColorFontEnabled: usize,
    SelectionHighlightColorWhenNotFocused: usize,
    SetSelectionHighlightColorWhenNotFocused: usize,
    HorizontalTextAlignment: usize,
    SetHorizontalTextAlignment: usize,
    CharacterCasing: usize,
    SetCharacterCasing: usize,
    PlaceholderForeground: usize,
    SetPlaceholderForeground: usize,
    CanPasteClipboardContent: usize,
    CanUndo: usize,
    CanRedo: usize,
    SelectionFlyout: usize,
    SetSelectionFlyout: usize,
    ProofingMenuFlyout: usize,
    Description: usize,
    SetDescription: usize,
    TextChanged: usize,
    RemoveTextChanged: usize,
    SelectionChanged: usize,
    RemoveSelectionChanged: usize,
    ContextMenuOpening: usize,
    RemoveContextMenuOpening: usize,
    Paste: usize,
    RemovePaste: usize,
    TextCompositionStarted: usize,
    RemoveTextCompositionStarted: usize,
    TextCompositionChanged: usize,
    RemoveTextCompositionChanged: usize,
    TextCompositionEnded: usize,
    RemoveTextCompositionEnded: usize,
    CopyingToClipboard: usize,
    RemoveCopyingToClipboard: usize,
    CuttingToClipboard: usize,
    RemoveCuttingToClipboard: usize,
    BeforeTextChanging: usize,
    RemoveBeforeTextChanging: usize,
    SelectionChanging: usize,
    RemoveSelectionChanging: usize,
    Select: usize,
    SelectAll: usize,
    pub GetRectFromCharacterIndex: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        i32,
        bool,
        *mut Rect,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITextBoxFactory,
    ITextBoxFactory_Vtbl,
    0xe1d8b82e_bc60_5d27_b646_5ca4c4a69432
);
impl windows_core::RuntimeType for ITextBoxFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITextBoxFactory");
}
#[repr(C)]
pub struct ITextBoxFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITextBoxStatics,
    ITextBoxStatics_Vtbl,
    0xa41cf38f_712a_5599_bbed_5a3d9b6bd46e
);
impl windows_core::RuntimeType for ITextBoxStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITextBoxStatics");
}
#[repr(C)]
pub struct ITextBoxStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITextElement,
    ITextElement_Vtbl,
    0xa122ba22_833f_5220_a47e_6cd507531abe
);
impl windows_core::RuntimeType for ITextElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.ITextElement");
}
#[repr(C)]
pub struct ITextElement_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Name: usize,
    FontSize: usize,
    SetFontSize: usize,
    FontFamily: usize,
    SetFontFamily: usize,
    FontWeight: usize,
    SetFontWeight: usize,
    FontStyle: usize,
    SetFontStyle: usize,
    FontStretch: usize,
    SetFontStretch: usize,
    CharacterSpacing: usize,
    SetCharacterSpacing: usize,
    Foreground: usize,
    SetForeground: usize,
    Language: usize,
    SetLanguage: usize,
    IsTextScaleFactorEnabled: usize,
    SetIsTextScaleFactorEnabled: usize,
    TextDecorations: usize,
    SetTextDecorations: usize,
    pub ContentStart: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITextElementFactory,
    ITextElementFactory_Vtbl,
    0xdf51fb95_a5e6_5b16_8e88_9f7cbfa234b1
);
impl windows_core::RuntimeType for ITextElementFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Documents.ITextElementFactory",
    );
}
#[repr(C)]
pub struct ITextElementFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITextElementStatics,
    ITextElementStatics_Vtbl,
    0xc9b55919_e1fe_5acd_bac7_c9d7f413b35c
);
impl windows_core::RuntimeType for ITextElementStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Documents.ITextElementStatics",
    );
}
#[repr(C)]
pub struct ITextElementStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITextHighlighter,
    ITextHighlighter_Vtbl,
    0xb756e861_1d2b_5f6f_81fd_c51a5bc068ff
);
impl windows_core::RuntimeType for ITextHighlighter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.ITextHighlighter");
}
#[repr(C)]
pub struct ITextHighlighter_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Ranges: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Foreground: usize,
    SetForeground: usize,
    Background: usize,
    pub SetBackground: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITextHighlighterFactory,
    ITextHighlighterFactory_Vtbl,
    0x69c7311f_c019_5b93_b511_81418543bab7
);
impl windows_core::RuntimeType for ITextHighlighterFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Documents.ITextHighlighterFactory",
    );
}
#[repr(C)]
pub struct ITextHighlighterFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITextHighlighterStatics,
    ITextHighlighterStatics_Vtbl,
    0x4975047a_87ad_51a2_977c_e771de4f4035
);
impl windows_core::RuntimeType for ITextHighlighterStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Documents.ITextHighlighterStatics",
    );
}
#[repr(C)]
pub struct ITextHighlighterStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITextPointer,
    ITextPointer_Vtbl,
    0x842eb385_ee41_5930_979b_438fa7525a51
);
impl windows_core::RuntimeType for ITextPointer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.ITextPointer");
}
#[repr(C)]
pub struct ITextPointer_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Parent: usize,
    VisualParent: usize,
    LogicalDirection: usize,
    pub Offset:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> windows_core::HRESULT,
    pub GetCharacterRect: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        LogicalDirection,
        *mut Rect,
    ) -> windows_core::HRESULT,
    pub GetPositionAtOffset: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        i32,
        LogicalDirection,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITreeView,
    ITreeView_Vtbl,
    0x1bef9af4_712c_50ef_9bb4_881b975232ab
);
impl windows_core::RuntimeType for ITreeView {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITreeView");
}
#[repr(C)]
pub struct ITreeView_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITreeView2,
    ITreeView2_Vtbl,
    0xb947ca7d_0f6f_594c_83ec_14153d343225
);
impl windows_core::RuntimeType for ITreeView2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITreeView2");
}
#[repr(C)]
pub struct ITreeView2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    NodeFromContainer: usize,
    ContainerFromNode: usize,
    ItemFromContainer: usize,
    ContainerFromItem: usize,
    CanDragItems: usize,
    SetCanDragItems: usize,
    CanReorderItems: usize,
    SetCanReorderItems: usize,
    ItemTemplate: usize,
    SetItemTemplate: usize,
    ItemTemplateSelector: usize,
    SetItemTemplateSelector: usize,
    ItemContainerStyle: usize,
    SetItemContainerStyle: usize,
    ItemContainerStyleSelector: usize,
    SetItemContainerStyleSelector: usize,
    ItemContainerTransitions: usize,
    SetItemContainerTransitions: usize,
    ItemsSource: usize,
    SetItemsSource: usize,
    pub DragItemsStarting: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveDragItemsStarting:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub DragItemsCompleted: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveDragItemsCompleted:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITreeViewDragItemsCompletedEventArgs,
    ITreeViewDragItemsCompletedEventArgs_Vtbl,
    0xe5b8547e_f839_55db_9c26_2a95f57a60dc
);
impl windows_core::RuntimeType for ITreeViewDragItemsCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.ITreeViewDragItemsCompletedEventArgs",
    );
}
#[repr(C)]
pub struct ITreeViewDragItemsCompletedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    DropResult: usize,
    pub Items: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITreeViewDragItemsCompletedEventArgs2,
    ITreeViewDragItemsCompletedEventArgs2_Vtbl,
    0x060d3921_57a0_5bc5_be26_320bd9c1a6e8
);
impl windows_core::RuntimeType for ITreeViewDragItemsCompletedEventArgs2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.ITreeViewDragItemsCompletedEventArgs2",
    );
}
#[repr(C)]
pub struct ITreeViewDragItemsCompletedEventArgs2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub NewParentItem: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITreeViewDragItemsStartingEventArgs,
    ITreeViewDragItemsStartingEventArgs_Vtbl,
    0x1b6c4ffc_cd32_5e06_b782_df9f077546c7
);
impl windows_core::RuntimeType for ITreeViewDragItemsStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.ITreeViewDragItemsStartingEventArgs",
    );
}
#[repr(C)]
pub struct ITreeViewDragItemsStartingEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Cancel:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetCancel: unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    Data: usize,
    pub Items: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITreeViewFactory,
    ITreeViewFactory_Vtbl,
    0x9c6220be_f9eb_518a_b30e_7e41de5efda9
);
impl windows_core::RuntimeType for ITreeViewFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITreeViewFactory");
}
#[repr(C)]
pub struct ITreeViewFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITreeViewNode,
    ITreeViewNode_Vtbl,
    0x00378a74_790b_5328_8afa_7d65e22da426
);
impl windows_core::RuntimeType for ITreeViewNode {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITreeViewNode");
}
#[repr(C)]
pub struct ITreeViewNode_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Content: usize,
    SetContent: usize,
    Parent: usize,
    IsExpanded: usize,
    SetIsExpanded: usize,
    HasChildren: usize,
    Depth: usize,
    HasUnrealizedChildren: usize,
    SetHasUnrealizedChildren: usize,
    pub Children: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITreeViewNodeFactory,
    ITreeViewNodeFactory_Vtbl,
    0xc105a5e5_cea8_5efd_8be8_3d89b54cbd5f
);
impl windows_core::RuntimeType for ITreeViewNodeFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.ITreeViewNodeFactory",
    );
}
#[repr(C)]
pub struct ITreeViewNodeFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITreeViewNodeStatics,
    ITreeViewNodeStatics_Vtbl,
    0xf5bb217c_66b9_5b62_b2d6_fb47843c8f6f
);
impl windows_core::RuntimeType for ITreeViewNodeStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Controls.ITreeViewNodeStatics",
    );
}
#[repr(C)]
pub struct ITreeViewNodeStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITreeViewStatics,
    ITreeViewStatics_Vtbl,
    0x71efaf86_4d73_5f5f_8ea6_a42dfe044917
);
impl windows_core::RuntimeType for ITreeViewStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITreeViewStatics");
}
#[repr(C)]
pub struct ITreeViewStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITreeViewStatics2,
    ITreeViewStatics2_Vtbl,
    0xddd01c44_a883_583b_9181_dca583da0ad9
);
impl windows_core::RuntimeType for ITreeViewStatics2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Controls.ITreeViewStatics2");
}
#[repr(C)]
pub struct ITreeViewStatics2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITypography,
    ITypography_Vtbl,
    0xfa27e2e3_be5e_5d21_9a5e_90cf102af828
);
impl windows_core::RuntimeType for ITypography {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.ITypography");
}
#[repr(C)]
pub struct ITypography_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITypographyStatics,
    ITypographyStatics_Vtbl,
    0x55fe4535_2125_533a_ada8_27be2b9e1193
);
impl windows_core::RuntimeType for ITypographyStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Documents.ITypographyStatics",
    );
}
#[repr(C)]
pub struct ITypographyStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    AnnotationAlternatesProperty: usize,
    GetAnnotationAlternates: usize,
    SetAnnotationAlternates: usize,
    EastAsianExpertFormsProperty: usize,
    GetEastAsianExpertForms: usize,
    SetEastAsianExpertForms: usize,
    EastAsianLanguageProperty: usize,
    GetEastAsianLanguage: usize,
    SetEastAsianLanguage: usize,
    EastAsianWidthsProperty: usize,
    GetEastAsianWidths: usize,
    SetEastAsianWidths: usize,
    StandardLigaturesProperty: usize,
    GetStandardLigatures: usize,
    SetStandardLigatures: usize,
    ContextualLigaturesProperty: usize,
    GetContextualLigatures: usize,
    SetContextualLigatures: usize,
    DiscretionaryLigaturesProperty: usize,
    GetDiscretionaryLigatures: usize,
    SetDiscretionaryLigatures: usize,
    HistoricalLigaturesProperty: usize,
    GetHistoricalLigatures: usize,
    SetHistoricalLigatures: usize,
    StandardSwashesProperty: usize,
    GetStandardSwashes: usize,
    SetStandardSwashes: usize,
    ContextualSwashesProperty: usize,
    GetContextualSwashes: usize,
    SetContextualSwashes: usize,
    ContextualAlternatesProperty: usize,
    GetContextualAlternates: usize,
    SetContextualAlternates: usize,
    StylisticAlternatesProperty: usize,
    GetStylisticAlternates: usize,
    SetStylisticAlternates: usize,
    StylisticSet1Property: usize,
    GetStylisticSet1: usize,
    SetStylisticSet1: usize,
    StylisticSet2Property: usize,
    GetStylisticSet2: usize,
    SetStylisticSet2: usize,
    StylisticSet3Property: usize,
    GetStylisticSet3: usize,
    SetStylisticSet3: usize,
    StylisticSet4Property: usize,
    GetStylisticSet4: usize,
    SetStylisticSet4: usize,
    StylisticSet5Property: usize,
    GetStylisticSet5: usize,
    SetStylisticSet5: usize,
    StylisticSet6Property: usize,
    GetStylisticSet6: usize,
    SetStylisticSet6: usize,
    StylisticSet7Property: usize,
    GetStylisticSet7: usize,
    SetStylisticSet7: usize,
    StylisticSet8Property: usize,
    GetStylisticSet8: usize,
    SetStylisticSet8: usize,
    StylisticSet9Property: usize,
    GetStylisticSet9: usize,
    SetStylisticSet9: usize,
    StylisticSet10Property: usize,
    GetStylisticSet10: usize,
    SetStylisticSet10: usize,
    StylisticSet11Property: usize,
    GetStylisticSet11: usize,
    SetStylisticSet11: usize,
    StylisticSet12Property: usize,
    GetStylisticSet12: usize,
    SetStylisticSet12: usize,
    StylisticSet13Property: usize,
    GetStylisticSet13: usize,
    SetStylisticSet13: usize,
    StylisticSet14Property: usize,
    GetStylisticSet14: usize,
    SetStylisticSet14: usize,
    StylisticSet15Property: usize,
    GetStylisticSet15: usize,
    SetStylisticSet15: usize,
    StylisticSet16Property: usize,
    GetStylisticSet16: usize,
    SetStylisticSet16: usize,
    StylisticSet17Property: usize,
    GetStylisticSet17: usize,
    SetStylisticSet17: usize,
    StylisticSet18Property: usize,
    GetStylisticSet18: usize,
    SetStylisticSet18: usize,
    StylisticSet19Property: usize,
    GetStylisticSet19: usize,
    SetStylisticSet19: usize,
    StylisticSet20Property: usize,
    GetStylisticSet20: usize,
    SetStylisticSet20: usize,
    CapitalsProperty: usize,
    GetCapitals: usize,
    SetCapitals: usize,
    CapitalSpacingProperty: usize,
    GetCapitalSpacing: usize,
    SetCapitalSpacing: usize,
    KerningProperty: usize,
    GetKerning: usize,
    SetKerning: usize,
    CaseSensitiveFormsProperty: usize,
    GetCaseSensitiveForms: usize,
    SetCaseSensitiveForms: usize,
    HistoricalFormsProperty: usize,
    GetHistoricalForms: usize,
    SetHistoricalForms: usize,
    FractionProperty: usize,
    GetFraction: usize,
    SetFraction: usize,
    NumeralStyleProperty: usize,
    GetNumeralStyle: usize,
    SetNumeralStyle: usize,
    NumeralAlignmentProperty: usize,
    GetNumeralAlignment: usize,
    SetNumeralAlignment: usize,
    SlashedZeroProperty: usize,
    GetSlashedZero: usize,
    SetSlashedZero: usize,
    MathematicalGreekProperty: usize,
    GetMathematicalGreek: usize,
    SetMathematicalGreek: usize,
    VariantsProperty: usize,
    GetVariants: usize,
    pub SetVariants: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        FontVariants,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IUIElement,
    IUIElement_Vtbl,
    0xc3c01020_320c_5cf6_9d24_d396bbfa4d8b
);
impl windows_core::RuntimeType for IUIElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IUIElement");
}
#[repr(C)]
pub struct IUIElement_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    DesiredSize: usize,
    AllowDrop: usize,
    pub SetAllowDrop:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    Opacity: usize,
    SetOpacity: usize,
    Clip: usize,
    SetClip: usize,
    RenderTransform: usize,
    SetRenderTransform: usize,
    Projection: usize,
    SetProjection: usize,
    Transform3D: usize,
    SetTransform3D: usize,
    RenderTransformOrigin: usize,
    SetRenderTransformOrigin: usize,
    IsHitTestVisible: usize,
    pub SetIsHitTestVisible:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    Visibility: usize,
    pub SetVisibility:
        unsafe extern "system" fn(*mut core::ffi::c_void, Visibility) -> windows_core::HRESULT,
    RenderSize: usize,
    UseLayoutRounding: usize,
    SetUseLayoutRounding: usize,
    Transitions: usize,
    SetTransitions: usize,
    CacheMode: usize,
    SetCacheMode: usize,
    IsTapEnabled: usize,
    SetIsTapEnabled: usize,
    IsDoubleTapEnabled: usize,
    SetIsDoubleTapEnabled: usize,
    CanDrag: usize,
    SetCanDrag: usize,
    IsRightTapEnabled: usize,
    SetIsRightTapEnabled: usize,
    IsHoldingEnabled: usize,
    SetIsHoldingEnabled: usize,
    ManipulationMode: usize,
    SetManipulationMode: usize,
    PointerCaptures: usize,
    pub ContextFlyout: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetContextFlyout: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    CompositeMode: usize,
    SetCompositeMode: usize,
    Lights: usize,
    CanBeScrollAnchor: usize,
    SetCanBeScrollAnchor: usize,
    ExitDisplayModeOnAccessKeyInvoked: usize,
    SetExitDisplayModeOnAccessKeyInvoked: usize,
    IsAccessKeyScope: usize,
    SetIsAccessKeyScope: usize,
    AccessKeyScopeOwner: usize,
    SetAccessKeyScopeOwner: usize,
    AccessKey: usize,
    SetAccessKey: usize,
    KeyTipPlacementMode: usize,
    SetKeyTipPlacementMode: usize,
    KeyTipHorizontalOffset: usize,
    SetKeyTipHorizontalOffset: usize,
    KeyTipVerticalOffset: usize,
    SetKeyTipVerticalOffset: usize,
    KeyTipTarget: usize,
    SetKeyTipTarget: usize,
    XYFocusKeyboardNavigation: usize,
    SetXYFocusKeyboardNavigation: usize,
    XYFocusUpNavigationStrategy: usize,
    SetXYFocusUpNavigationStrategy: usize,
    XYFocusDownNavigationStrategy: usize,
    SetXYFocusDownNavigationStrategy: usize,
    XYFocusLeftNavigationStrategy: usize,
    SetXYFocusLeftNavigationStrategy: usize,
    XYFocusRightNavigationStrategy: usize,
    SetXYFocusRightNavigationStrategy: usize,
    KeyboardAccelerators: usize,
    KeyboardAcceleratorPlacementTarget: usize,
    SetKeyboardAcceleratorPlacementTarget: usize,
    KeyboardAcceleratorPlacementMode: usize,
    SetKeyboardAcceleratorPlacementMode: usize,
    HighContrastAdjustment: usize,
    SetHighContrastAdjustment: usize,
    TabFocusNavigation: usize,
    SetTabFocusNavigation: usize,
    OpacityTransition: usize,
    SetOpacityTransition: usize,
    Translation: usize,
    SetTranslation: usize,
    TranslationTransition: usize,
    SetTranslationTransition: usize,
    Rotation: usize,
    SetRotation: usize,
    RotationTransition: usize,
    SetRotationTransition: usize,
    Scale: usize,
    SetScale: usize,
    ScaleTransition: usize,
    SetScaleTransition: usize,
    TransformMatrix: usize,
    SetTransformMatrix: usize,
    CenterPoint: usize,
    SetCenterPoint: usize,
    RotationAxis: usize,
    SetRotationAxis: usize,
    ActualOffset: usize,
    ActualSize: usize,
    pub XamlRoot: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetXamlRoot: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Shadow: usize,
    SetShadow: usize,
    RasterizationScale: usize,
    SetRasterizationScale: usize,
    FocusState: usize,
    UseSystemFocusVisuals: usize,
    SetUseSystemFocusVisuals: usize,
    XYFocusLeft: usize,
    SetXYFocusLeft: usize,
    XYFocusRight: usize,
    SetXYFocusRight: usize,
    XYFocusUp: usize,
    SetXYFocusUp: usize,
    XYFocusDown: usize,
    SetXYFocusDown: usize,
    pub IsTabStop:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetIsTabStop:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    TabIndex: usize,
    SetTabIndex: usize,
    KeyUp: usize,
    RemoveKeyUp: usize,
    pub KeyDown: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveKeyDown:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    GotFocus: usize,
    RemoveGotFocus: usize,
    LostFocus: usize,
    RemoveLostFocus: usize,
    DragStarting: usize,
    RemoveDragStarting: usize,
    DropCompleted: usize,
    RemoveDropCompleted: usize,
    CharacterReceived: usize,
    RemoveCharacterReceived: usize,
    DragEnter: usize,
    RemoveDragEnter: usize,
    DragLeave: usize,
    RemoveDragLeave: usize,
    pub DragOver: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveDragOver:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub Drop: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveDrop: unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub PointerPressed: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemovePointerPressed:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub PointerMoved: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemovePointerMoved:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub PointerReleased: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemovePointerReleased:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    PointerEntered: usize,
    RemovePointerEntered: usize,
    PointerExited: usize,
    RemovePointerExited: usize,
    PointerCaptureLost: usize,
    RemovePointerCaptureLost: usize,
    PointerCanceled: usize,
    RemovePointerCanceled: usize,
    pub PointerWheelChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemovePointerWheelChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    Tapped: usize,
    RemoveTapped: usize,
    pub DoubleTapped: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveDoubleTapped:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    Holding: usize,
    RemoveHolding: usize,
    pub ContextRequested: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveContextRequested:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    ContextCanceled: usize,
    RemoveContextCanceled: usize,
    RightTapped: usize,
    RemoveRightTapped: usize,
    ManipulationStarting: usize,
    RemoveManipulationStarting: usize,
    ManipulationInertiaStarting: usize,
    RemoveManipulationInertiaStarting: usize,
    ManipulationStarted: usize,
    RemoveManipulationStarted: usize,
    ManipulationDelta: usize,
    RemoveManipulationDelta: usize,
    ManipulationCompleted: usize,
    RemoveManipulationCompleted: usize,
    AccessKeyDisplayRequested: usize,
    RemoveAccessKeyDisplayRequested: usize,
    AccessKeyDisplayDismissed: usize,
    RemoveAccessKeyDisplayDismissed: usize,
    AccessKeyInvoked: usize,
    RemoveAccessKeyInvoked: usize,
    ProcessKeyboardAccelerators: usize,
    RemoveProcessKeyboardAccelerators: usize,
    GettingFocus: usize,
    RemoveGettingFocus: usize,
    LosingFocus: usize,
    RemoveLosingFocus: usize,
    NoFocusCandidateFound: usize,
    RemoveNoFocusCandidateFound: usize,
    pub PreviewKeyDown: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemovePreviewKeyDown:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    PreviewKeyUp: usize,
    RemovePreviewKeyUp: usize,
    BringIntoViewRequested: usize,
    RemoveBringIntoViewRequested: usize,
    Measure: usize,
    Arrange: usize,
    pub CapturePointer: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut bool,
    ) -> windows_core::HRESULT,
    pub ReleasePointerCapture: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    ReleasePointerCaptures: usize,
    AddHandler: usize,
    RemoveHandler: usize,
    TransformToVisual: usize,
    InvalidateMeasure: usize,
    InvalidateArrange: usize,
    UpdateLayout: usize,
    CancelDirectManipulations: usize,
    StartDragAsync: usize,
    pub StartBringIntoView:
        unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub StartBringIntoViewWithOptions: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    TryInvokeKeyboardAccelerator: usize,
    pub Focus: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        FocusState,
        *mut bool,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IUIElementProtected,
    IUIElementProtected_Vtbl,
    0x8f69b9e9_1f00_5834_9bf1_a9257bed39f0
);
impl windows_core::RuntimeType for IUIElementProtected {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IUIElementProtected");
}
#[repr(C)]
pub struct IUIElementProtected_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    ProtectedCursor: usize,
    pub SetProtectedCursor: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IUIElementStatics,
    IUIElementStatics_Vtbl,
    0xd2921d87_3584_5e22_8a3a_c2c78dab4f6e
);
impl windows_core::RuntimeType for IUIElementStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IUIElementStatics");
}
#[repr(C)]
pub struct IUIElementStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IVisualTreeHelper,
    IVisualTreeHelper_Vtbl,
    0x5f69ac1e_6504_5e3f_a11c_87684c1db814
);
impl windows_core::RuntimeType for IVisualTreeHelper {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Media.IVisualTreeHelper");
}
#[repr(C)]
pub struct IVisualTreeHelper_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IVisualTreeHelperStatics,
    IVisualTreeHelperStatics_Vtbl,
    0x5aece43c_7651_5bb5_855c_2198496e455e
);
impl windows_core::RuntimeType for IVisualTreeHelperStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Media.IVisualTreeHelperStatics",
    );
}
#[repr(C)]
pub struct IVisualTreeHelperStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    FindElementsInHostCoordinatesPoint: usize,
    FindElementsInHostCoordinatesRect: usize,
    FindAllElementsInHostCoordinatesPoint: usize,
    FindAllElementsInHostCoordinatesRect: usize,
    pub GetChild: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        i32,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetChildrenCount: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i32,
    ) -> windows_core::HRESULT,
    pub GetParent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IXamlReader,
    IXamlReader_Vtbl,
    0x54ce54c8_38c6_50d9_ac98_4b03eddbde9f
);
impl windows_core::RuntimeType for IXamlReader {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Markup.IXamlReader");
}
#[repr(C)]
pub struct IXamlReader_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IXamlReaderStatics,
    IXamlReaderStatics_Vtbl,
    0x82a4cd9e_435e_5aeb_8c4f_300cece45cae
);
impl windows_core::RuntimeType for IXamlReaderStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Markup.IXamlReaderStatics");
}
#[repr(C)]
pub struct IXamlReaderStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Load: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IXamlRoot,
    IXamlRoot_Vtbl,
    0x60cb215a_ad15_520a_8b01_4416824f0441
);
impl windows_core::RuntimeType for IXamlRoot {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IXamlRoot");
}
#[repr(C)]
pub struct IXamlRoot_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IXamlRoot2,
    IXamlRoot2_Vtbl,
    0xbdee0f42_71cb_50c5_829b_4614d98c5794
);
impl windows_core::RuntimeType for IXamlRoot2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IXamlRoot2");
}
#[repr(C)]
pub struct IXamlRoot2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub ContentIslandEnvironment: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Inline(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Inline, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(Inline, TextElement, DependencyObject);
impl Inline {
    pub fn ContentStart(&self) -> windows_core::Result<TextPointer> {
        let this = &windows_core::Interface::cast::<ITextElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContentStart)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    fn IInlineFactory<R, F: FnOnce(&IInlineFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Inline, IInlineFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Inline {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IInline>();
}
unsafe impl windows_core::Interface for Inline {
    type Vtable = <IInline as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IInline as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for Inline {
    const NAME: &'static str = "Microsoft.UI.Xaml.Documents.Inline";
}
unsafe impl Send for Inline {}
unsafe impl Sync for Inline {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineCollection(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    InlineCollection,
    windows_core::IUnknown,
    windows_core::IInspectable,
    windows_collections::IVector<Inline>
);
impl windows_core::RuntimeType for InlineCollection {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, windows_collections::IVector<Inline>>();
}
unsafe impl windows_core::Interface for InlineCollection {
    type Vtable = <windows_collections::IVector<Inline> as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <windows_collections::IVector<Inline> as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for InlineCollection {
    const NAME: &'static str = "Microsoft.UI.Xaml.Documents.InlineCollection";
}
unsafe impl Send for InlineCollection {}
unsafe impl Sync for InlineCollection {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineUIContainer(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    InlineUIContainer,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(InlineUIContainer, Inline, TextElement, DependencyObject);
impl InlineUIContainer {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            InlineUIContainer,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn SetChild<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<UIElement>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetChild)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ContentStart(&self) -> windows_core::Result<TextPointer> {
        let this = &windows_core::Interface::cast::<ITextElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContentStart)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for InlineUIContainer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IInlineUIContainer>();
}
unsafe impl windows_core::Interface for InlineUIContainer {
    type Vtable = <IInlineUIContainer as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IInlineUIContainer as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for InlineUIContainer {
    const NAME: &'static str = "Microsoft.UI.Xaml.Documents.InlineUIContainer";
}
unsafe impl Send for InlineUIContainer {}
unsafe impl Sync for InlineUIContainer {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputCursor(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    InputCursor,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl InputCursor {
    fn IInputCursorStatics<R, F: FnOnce(&IInputCursorStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<InputCursor, IInputCursorStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for InputCursor {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IInputCursor>();
}
unsafe impl windows_core::Interface for InputCursor {
    type Vtable = <IInputCursor as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IInputCursor as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for InputCursor {
    const NAME: &'static str = "Microsoft.UI.Input.InputCursor";
}
unsafe impl Send for InputCursor {}
unsafe impl Sync for InputCursor {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputKeyboardSource(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    InputKeyboardSource,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(InputKeyboardSource, InputObject);
impl InputKeyboardSource {
    pub fn GetKeyStateForCurrentThread(
        virtualkey: VirtualKey,
    ) -> windows_core::Result<CoreVirtualKeyStates> {
        Self::IInputKeyboardSourceStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetKeyStateForCurrentThread)(
                windows_core::Interface::as_raw(this),
                virtualkey,
                &mut result__,
            )
            .map(|| result__)
        })
    }
    fn IInputKeyboardSourceStatics<
        R,
        F: FnOnce(&IInputKeyboardSourceStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            InputKeyboardSource,
            IInputKeyboardSourceStatics,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IInputKeyboardSourceStatics2<
        R,
        F: FnOnce(&IInputKeyboardSourceStatics2) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            InputKeyboardSource,
            IInputKeyboardSourceStatics2,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IInputKeyboardSourceStatics3<
        R,
        F: FnOnce(&IInputKeyboardSourceStatics3) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            InputKeyboardSource,
            IInputKeyboardSourceStatics3,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for InputKeyboardSource {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IInputKeyboardSource>();
}
unsafe impl windows_core::Interface for InputKeyboardSource {
    type Vtable = <IInputKeyboardSource as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IInputKeyboardSource as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for InputKeyboardSource {
    const NAME: &'static str = "Microsoft.UI.Input.InputKeyboardSource";
}
unsafe impl Send for InputKeyboardSource {}
unsafe impl Sync for InputKeyboardSource {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputObject(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    InputObject,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for InputObject {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IInputObject>();
}
unsafe impl windows_core::Interface for InputObject {
    type Vtable = <IInputObject as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IInputObject as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for InputObject {
    const NAME: &'static str = "Microsoft.UI.Input.InputObject";
}
unsafe impl Send for InputObject {}
unsafe impl Sync for InputObject {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputSystemCursor(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    InputSystemCursor,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(InputSystemCursor, InputCursor);
impl InputSystemCursor {
    pub fn Create(r#type: InputSystemCursorShape) -> windows_core::Result<Self> {
        Self::IInputSystemCursorStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Create)(
                windows_core::Interface::as_raw(this),
                r#type,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IInputSystemCursorStatics<
        R,
        F: FnOnce(&IInputSystemCursorStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            InputSystemCursor,
            IInputSystemCursorStatics,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for InputSystemCursor {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IInputSystemCursor>();
}
unsafe impl windows_core::Interface for InputSystemCursor {
    type Vtable = <IInputSystemCursor as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IInputSystemCursor as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for InputSystemCursor {
    const NAME: &'static str = "Microsoft.UI.Input.InputSystemCursor";
}
unsafe impl Send for InputSystemCursor {}
unsafe impl Sync for InputSystemCursor {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InputSystemCursorShape(pub i32);
impl InputSystemCursorShape {
    pub const Arrow: Self = Self(0);
    pub const Cross: Self = Self(1);
    pub const Hand: Self = Self(3);
    pub const Help: Self = Self(4);
    pub const IBeam: Self = Self(5);
    pub const SizeAll: Self = Self(6);
    pub const SizeNortheastSouthwest: Self = Self(7);
    pub const SizeNorthSouth: Self = Self(8);
    pub const SizeNorthwestSoutheast: Self = Self(9);
    pub const SizeWestEast: Self = Self(10);
    pub const UniversalNo: Self = Self(11);
    pub const UpArrow: Self = Self(12);
    pub const Wait: Self = Self(13);
    pub const Pin: Self = Self(14);
    pub const Person: Self = Self(15);
    pub const AppStarting: Self = Self(16);
}
impl windows_core::imp::TypeKind for InputSystemCursorShape {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for InputSystemCursorShape {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Input.InputSystemCursorShape;i4)",
    );
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Input.InputSystemCursorShape");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemsControl(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ItemsControl,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ItemsControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl ItemsControl {
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn IItemsControlFactory<R, F: FnOnce(&IItemsControlFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ItemsControl, IItemsControlFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IItemsControlStatics<R, F: FnOnce(&IItemsControlStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ItemsControl, IItemsControlStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ItemsControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IItemsControl>();
}
unsafe impl windows_core::Interface for ItemsControl {
    type Vtable = <IItemsControl as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IItemsControl as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for ItemsControl {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ItemsControl";
}
unsafe impl Send for ItemsControl {}
unsafe impl Sync for ItemsControl {}
windows_core::imp::define_interface!(
    KeyEventHandler,
    KeyEventHandler_Vtbl,
    0xdb68e7cc_9a2b_527d_9989_25284daccc03
);
impl windows_core::RuntimeType for KeyEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl KeyEventHandler {
    pub fn new<
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<KeyRoutedEventArgs>,
            ) -> windows_core::Result<()>
            + Send
            + 'static,
    >(
        invoke: F,
    ) -> Self {
        let com = windows_core::imp::DelegateBox::<Self, F>::new(
            &KeyEventHandlerBox::<F>::VTABLE,
            invoke,
        );
        unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
    }
    pub fn Invoke<P0, P1>(&self, sender: P0, e: P1) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
        P1: windows_core::Param<KeyRoutedEventArgs>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).Invoke)(
                windows_core::Interface::as_raw(self),
                sender.param().abi(),
                e.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct KeyEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct KeyEventHandlerBox<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<KeyRoutedEventArgs>,
        ) -> windows_core::Result<()>
        + Send
        + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<KeyRoutedEventArgs>,
        ) -> windows_core::Result<()>
        + Send
        + 'static,
> KeyEventHandlerBox<F>
{
    const VTABLE: KeyEventHandler_Vtbl = KeyEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface: windows_core::imp::DelegateBox::<KeyEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<KeyEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<KeyEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<KeyEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            )
            .into()
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyRoutedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    KeyRoutedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(KeyRoutedEventArgs, RoutedEventArgs);
impl KeyRoutedEventArgs {
    pub fn Key(&self) -> windows_core::Result<VirtualKey> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Key)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Handled(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Handled)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetHandled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHandled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn OriginalKey(&self) -> windows_core::Result<VirtualKey> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).OriginalKey)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn OriginalSource(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IRoutedEventArgs>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).OriginalSource)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for KeyRoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IKeyRoutedEventArgs>();
}
unsafe impl windows_core::Interface for KeyRoutedEventArgs {
    type Vtable = <IKeyRoutedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IKeyRoutedEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for KeyRoutedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Input.KeyRoutedEventArgs";
}
unsafe impl Send for KeyRoutedEventArgs {}
unsafe impl Sync for KeyRoutedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LineBreak(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    LineBreak,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(LineBreak, Inline, TextElement, DependencyObject);
impl LineBreak {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            LineBreak,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn ContentStart(&self) -> windows_core::Result<TextPointer> {
        let this = &windows_core::Interface::cast::<ITextElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContentStart)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for LineBreak {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ILineBreak>();
}
unsafe impl windows_core::Interface for LineBreak {
    type Vtable = <ILineBreak as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ILineBreak as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for LineBreak {
    const NAME: &'static str = "Microsoft.UI.Xaml.Documents.LineBreak";
}
unsafe impl Send for LineBreak {}
unsafe impl Sync for LineBreak {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListViewBase(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ListViewBase,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ListViewBase,
    Selector,
    ItemsControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl ListViewBase {
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SelectedItems(
        &self,
    ) -> windows_core::Result<windows_collections::IVector<windows_core::IInspectable>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SelectedItems)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn DragItemsStarting<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DragItemsStartingEventArgs>,
            ) + Send
            + 'static,
    {
        let handler = <DragItemsStartingEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).DragItemsStarting)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveDragItemsStarting,
            ))
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn IListViewBaseFactory<R, F: FnOnce(&IListViewBaseFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ListViewBase, IListViewBaseFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IListViewBaseStatics<R, F: FnOnce(&IListViewBaseStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ListViewBase, IListViewBaseStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ListViewBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IListViewBase>();
}
unsafe impl windows_core::Interface for ListViewBase {
    type Vtable = <IListViewBase as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IListViewBase as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for ListViewBase {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ListViewBase";
}
unsafe impl Send for ListViewBase {}
unsafe impl Sync for ListViewBase {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListViewItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ListViewItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ListViewItem,
    SelectorItem,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl ListViewItem {
    pub fn Content(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Content)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContent)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetContentTemplate<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DataTemplate>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContentTemplate)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn IListViewItemFactory<R, F: FnOnce(&IListViewItemFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ListViewItem, IListViewItemFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ListViewItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IListViewItem>();
}
unsafe impl windows_core::Interface for ListViewItem {
    type Vtable = <IListViewItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IListViewItem as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for ListViewItem {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ListViewItem";
}
unsafe impl Send for ListViewItem {}
unsafe impl Sync for ListViewItem {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LogicalDirection(pub i32);
impl LogicalDirection {
    pub const Backward: Self = Self(0);
    pub const Forward: Self = Self(1);
}
impl windows_core::imp::TypeKind for LogicalDirection {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for LogicalDirection {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Xaml.Documents.LogicalDirection;i4)",
    );
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.LogicalDirection");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Paragraph(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Paragraph,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(Paragraph, Block, TextElement, DependencyObject);
impl Paragraph {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            Paragraph,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn Inlines(&self) -> windows_core::Result<InlineCollection> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Inlines)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn ContentStart(&self) -> windows_core::Result<TextPointer> {
        let this = &windows_core::Interface::cast::<ITextElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContentStart)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    fn IParagraphStatics<R, F: FnOnce(&IParagraphStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Paragraph, IParagraphStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Paragraph {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IParagraph>();
}
unsafe impl windows_core::Interface for Paragraph {
    type Vtable = <IParagraph as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IParagraph as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for Paragraph {
    const NAME: &'static str = "Microsoft.UI.Xaml.Documents.Paragraph";
}
unsafe impl Send for Paragraph {}
unsafe impl Sync for Paragraph {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub X: f32,
    pub Y: f32,
}
impl windows_core::imp::TypeKind for Point {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for Point {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"struct(Windows.Foundation.Point;f4;f4)");
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Windows.Foundation.Point");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pointer(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Pointer,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for Pointer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IPointer>();
}
unsafe impl windows_core::Interface for Pointer {
    type Vtable = <IPointer as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IPointer as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for Pointer {
    const NAME: &'static str = "Microsoft.UI.Xaml.Input.Pointer";
}
unsafe impl Send for Pointer {}
unsafe impl Sync for Pointer {}
windows_core::imp::define_interface!(
    PointerEventHandler,
    PointerEventHandler_Vtbl,
    0xa48a71e1_8bb4_5597_9e31_903a3f6a04fb
);
impl windows_core::RuntimeType for PointerEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl PointerEventHandler {
    pub fn new<
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) -> windows_core::Result<()>
            + Send
            + 'static,
    >(
        invoke: F,
    ) -> Self {
        let com = windows_core::imp::DelegateBox::<Self, F>::new(
            &PointerEventHandlerBox::<F>::VTABLE,
            invoke,
        );
        unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
    }
    pub fn Invoke<P0, P1>(&self, sender: P0, e: P1) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
        P1: windows_core::Param<PointerRoutedEventArgs>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).Invoke)(
                windows_core::Interface::as_raw(self),
                sender.param().abi(),
                e.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct PointerEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct PointerEventHandlerBox<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<PointerRoutedEventArgs>,
        ) -> windows_core::Result<()>
        + Send
        + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<PointerRoutedEventArgs>,
        ) -> windows_core::Result<()>
        + Send
        + 'static,
> PointerEventHandlerBox<F>
{
    const VTABLE: PointerEventHandler_Vtbl = PointerEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface:
                windows_core::imp::DelegateBox::<PointerEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<PointerEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<PointerEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<PointerEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            )
            .into()
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointerPoint(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    PointerPoint,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl PointerPoint {
    pub fn Position(&self) -> windows_core::Result<Point> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Position)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Properties(&self) -> windows_core::Result<PointerPointProperties> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Properties)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn Timestamp(&self) -> windows_core::Result<u64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Timestamp)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    fn IPointerPointStatics<R, F: FnOnce(&IPointerPointStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<PointerPoint, IPointerPointStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for PointerPoint {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IPointerPoint>();
}
unsafe impl windows_core::Interface for PointerPoint {
    type Vtable = <IPointerPoint as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IPointerPoint as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for PointerPoint {
    const NAME: &'static str = "Microsoft.UI.Input.PointerPoint";
}
unsafe impl Send for PointerPoint {}
unsafe impl Sync for PointerPoint {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointerPointProperties(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    PointerPointProperties,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl PointerPointProperties {
    pub fn IsHorizontalMouseWheel(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsHorizontalMouseWheel)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsLeftButtonPressed(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsLeftButtonPressed)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsMiddleButtonPressed(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsMiddleButtonPressed)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsRightButtonPressed(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsRightButtonPressed)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn MouseWheelDelta(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).MouseWheelDelta)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
impl windows_core::RuntimeType for PointerPointProperties {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IPointerPointProperties>();
}
unsafe impl windows_core::Interface for PointerPointProperties {
    type Vtable = <IPointerPointProperties as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IPointerPointProperties as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for PointerPointProperties {
    const NAME: &'static str = "Microsoft.UI.Input.PointerPointProperties";
}
unsafe impl Send for PointerPointProperties {}
unsafe impl Sync for PointerPointProperties {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointerRoutedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    PointerRoutedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(PointerRoutedEventArgs, RoutedEventArgs);
impl PointerRoutedEventArgs {
    pub fn Pointer(&self) -> windows_core::Result<Pointer> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Pointer)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn KeyModifiers(&self) -> windows_core::Result<VirtualKeyModifiers> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).KeyModifiers)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Handled(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Handled)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetHandled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHandled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn GetCurrentPoint<P0>(&self, relativeto: P0) -> windows_core::Result<PointerPoint>
    where
        P0: windows_core::Param<UIElement>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetCurrentPoint)(
                windows_core::Interface::as_raw(self),
                relativeto.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn OriginalSource(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IRoutedEventArgs>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).OriginalSource)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for PointerRoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IPointerRoutedEventArgs>();
}
unsafe impl windows_core::Interface for PointerRoutedEventArgs {
    type Vtable = <IPointerRoutedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IPointerRoutedEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for PointerRoutedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Input.PointerRoutedEventArgs";
}
unsafe impl Send for PointerRoutedEventArgs {}
unsafe impl Sync for PointerRoutedEventArgs {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub X: f32,
    pub Y: f32,
    pub Width: f32,
    pub Height: f32,
}
impl windows_core::imp::TypeKind for Rect {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for Rect {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"struct(Windows.Foundation.Rect;f4;f4;f4;f4)");
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Windows.Foundation.Rect");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceDictionary(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ResourceDictionary,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(ResourceDictionary, DependencyObject);
impl ResourceDictionary {
    fn IResourceDictionaryFactory<
        R,
        F: FnOnce(&IResourceDictionaryFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            ResourceDictionary,
            IResourceDictionaryFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ResourceDictionary {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IResourceDictionary>();
}
unsafe impl windows_core::Interface for ResourceDictionary {
    type Vtable = <IResourceDictionary as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IResourceDictionary as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for ResourceDictionary {
    const NAME: &'static str = "Microsoft.UI.Xaml.ResourceDictionary";
}
unsafe impl Send for ResourceDictionary {}
unsafe impl Sync for ResourceDictionary {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RichTextBlock(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    RichTextBlock,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    RichTextBlock,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl RichTextBlock {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            RichTextBlock,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Blocks(&self) -> windows_core::Result<BlockCollection> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Blocks)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn TextHighlighters(
        &self,
    ) -> windows_core::Result<windows_collections::IVector<TextHighlighter>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).TextHighlighters)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn IRichTextBlockStatics<R, F: FnOnce(&IRichTextBlockStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<RichTextBlock, IRichTextBlockStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for RichTextBlock {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IRichTextBlock>();
}
unsafe impl windows_core::Interface for RichTextBlock {
    type Vtable = <IRichTextBlock as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IRichTextBlock as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for RichTextBlock {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.RichTextBlock";
}
unsafe impl Send for RichTextBlock {}
unsafe impl Sync for RichTextBlock {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    RoutedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl RoutedEventArgs {
    pub fn OriginalSource(&self) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).OriginalSource)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    fn IRoutedEventArgsFactory<
        R,
        F: FnOnce(&IRoutedEventArgsFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<RoutedEventArgs, IRoutedEventArgsFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for RoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IRoutedEventArgs>();
}
unsafe impl windows_core::Interface for RoutedEventArgs {
    type Vtable = <IRoutedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IRoutedEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for RoutedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.RoutedEventArgs";
}
unsafe impl Send for RoutedEventArgs {}
unsafe impl Sync for RoutedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Run(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Run, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(Run, Inline, TextElement, DependencyObject);
impl Run {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Run, windows_core::imp::IGenericFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn Text(&self) -> windows_core::Result<windows_core::HSTRING> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Text)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn SetText(&self, value: &windows_core::HSTRING) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetText)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(value),
            )
            .ok()
        }
    }
    pub fn ContentStart(&self) -> windows_core::Result<TextPointer> {
        let this = &windows_core::Interface::cast::<ITextElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContentStart)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    fn IRunStatics<R, F: FnOnce(&IRunStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Run, IRunStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Run {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IRun>();
}
unsafe impl windows_core::Interface for Run {
    type Vtable = <IRun as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IRun as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for Run {
    const NAME: &'static str = "Microsoft.UI.Xaml.Documents.Run";
}
unsafe impl Send for Run {}
unsafe impl Sync for Run {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScrollViewer(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ScrollViewer,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ScrollViewer,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl ScrollViewer {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            ScrollViewer,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn Content(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Content)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContent)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetContentTemplate<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DataTemplate>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContentTemplate)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ViewportHeight(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ViewportHeight)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ExtentHeight(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ExtentHeight)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ScrollToVerticalOffset(&self, offset: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).ScrollToVerticalOffset)(
                windows_core::Interface::as_raw(self),
                offset,
            )
            .ok()
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn IScrollViewerStatics<R, F: FnOnce(&IScrollViewerStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ScrollViewer, IScrollViewerStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ScrollViewer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IScrollViewer>();
}
unsafe impl windows_core::Interface for ScrollViewer {
    type Vtable = <IScrollViewer as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IScrollViewer as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for ScrollViewer {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ScrollViewer";
}
unsafe impl Send for ScrollViewer {}
unsafe impl Sync for ScrollViewer {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selector(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Selector,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    Selector,
    ItemsControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl Selector {
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn ISelectorStatics<R, F: FnOnce(&ISelectorStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Selector, ISelectorStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Selector {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISelector>();
}
unsafe impl windows_core::Interface for Selector {
    type Vtable = <ISelector as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISelector as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for Selector {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.Selector";
}
unsafe impl Send for Selector {}
unsafe impl Sync for Selector {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectorItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    SelectorItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    SelectorItem,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl SelectorItem {
    pub fn Content(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Content)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContent)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetContentTemplate<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DataTemplate>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContentTemplate)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn ISelectorItemFactory<R, F: FnOnce(&ISelectorItemFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<SelectorItem, ISelectorItemFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn ISelectorItemStatics<R, F: FnOnce(&ISelectorItemStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<SelectorItem, ISelectorItemStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for SelectorItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISelectorItem>();
}
unsafe impl windows_core::Interface for SelectorItem {
    type Vtable = <ISelectorItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISelectorItem as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for SelectorItem {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.SelectorItem";
}
unsafe impl Send for SelectorItem {}
unsafe impl Sync for SelectorItem {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SolidColorBrush(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    SolidColorBrush,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(SolidColorBrush, Brush, DependencyObject);
impl SolidColorBrush {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            SolidColorBrush,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn Color(&self) -> windows_core::Result<Color> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Color)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetColor(&self, value: Color) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetColor)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn CreateInstanceWithColor(color: Color) -> windows_core::Result<Self> {
        Self::ISolidColorBrushFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstanceWithColor)(
                windows_core::Interface::as_raw(this),
                color,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn ISolidColorBrushFactory<
        R,
        F: FnOnce(&ISolidColorBrushFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<SolidColorBrush, ISolidColorBrushFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn ISolidColorBrushStatics<
        R,
        F: FnOnce(&ISolidColorBrushStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<SolidColorBrush, ISolidColorBrushStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for SolidColorBrush {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISolidColorBrush>();
}
unsafe impl windows_core::Interface for SolidColorBrush {
    type Vtable = <ISolidColorBrush as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISolidColorBrush as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for SolidColorBrush {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.SolidColorBrush";
}
unsafe impl Send for SolidColorBrush {}
unsafe impl Sync for SolidColorBrush {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Span(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Span, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(Span, Inline, TextElement, DependencyObject);
impl Span {
    pub fn Inlines(&self) -> windows_core::Result<InlineCollection> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Inlines)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetInlines<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InlineCollection>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetInlines)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ContentStart(&self) -> windows_core::Result<TextPointer> {
        let this = &windows_core::Interface::cast::<ITextElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContentStart)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    fn ISpanFactory<R, F: FnOnce(&ISpanFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Span, ISpanFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Span {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISpan>();
}
unsafe impl windows_core::Interface for Span {
    type Vtable = <ISpan as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISpan as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for Span {
    const NAME: &'static str = "Microsoft.UI.Xaml.Documents.Span";
}
unsafe impl Send for Span {}
unsafe impl Sync for Span {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemBackdropConfiguration(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    SystemBackdropConfiguration,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl SystemBackdropConfiguration {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            SystemBackdropConfiguration,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn SetIsInputActive(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsInputActive)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetTheme(&self, value: SystemBackdropTheme) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetTheme)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
impl windows_core::RuntimeType for SystemBackdropConfiguration {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISystemBackdropConfiguration>();
}
unsafe impl windows_core::Interface for SystemBackdropConfiguration {
    type Vtable = <ISystemBackdropConfiguration as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISystemBackdropConfiguration as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for SystemBackdropConfiguration {
    const NAME: &'static str =
        "Microsoft.UI.Composition.SystemBackdrops.SystemBackdropConfiguration";
}
unsafe impl Send for SystemBackdropConfiguration {}
unsafe impl Sync for SystemBackdropConfiguration {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SystemBackdropTheme(pub i32);
impl SystemBackdropTheme {
    pub const Default: Self = Self(0);
    pub const Light: Self = Self(1);
    pub const Dark: Self = Self(2);
}
impl windows_core::imp::TypeKind for SystemBackdropTheme {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for SystemBackdropTheme {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Composition.SystemBackdrops.SystemBackdropTheme;i4)",
    );
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Composition.SystemBackdrops.SystemBackdropTheme",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabView(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TabView,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    TabView,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl TabView {
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn TabDroppedOutside<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<Self>, windows_core::Ref<TabViewTabDroppedOutsideEventArgs>)
            + Send
            + 'static,
    {
        let handler =
            <TypedEventHandler<Self, TabViewTabDroppedOutsideEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).TabDroppedOutside)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveTabDroppedOutside,
            ))
        }
    }
    pub fn SetCanDragTabs(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetCanDragTabs)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetAllowDropTabs(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetAllowDropTabs)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn TabDragStarting<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<Self>, windows_core::Ref<TabViewTabDragStartingEventArgs>)
            + Send
            + 'static,
    {
        let handler =
            <TypedEventHandler<Self, TabViewTabDragStartingEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).TabDragStarting)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveTabDragStarting,
            ))
        }
    }
    pub fn TabDragCompleted<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<Self>, windows_core::Ref<TabViewTabDragCompletedEventArgs>)
            + Send
            + 'static,
    {
        let handler =
            <TypedEventHandler<Self, TabViewTabDragCompletedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).TabDragCompleted)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveTabDragCompleted,
            ))
        }
    }
    pub fn TabStripDragOver<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).TabStripDragOver)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveTabStripDragOver,
            ))
        }
    }
    pub fn TabStripDrop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).TabStripDrop)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveTabStripDrop,
            ))
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn ITabViewFactory<R, F: FnOnce(&ITabViewFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TabView, ITabViewFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn ITabViewStatics<R, F: FnOnce(&ITabViewStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TabView, ITabViewStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn ITabViewStatics2<R, F: FnOnce(&ITabViewStatics2) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TabView, ITabViewStatics2> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for TabView {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITabView>();
}
unsafe impl windows_core::Interface for TabView {
    type Vtable = <ITabView as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITabView as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TabView {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TabView";
}
unsafe impl Send for TabView {}
unsafe impl Sync for TabView {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabViewItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TabViewItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    TabViewItem,
    ListViewItem,
    SelectorItem,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl TabViewItem {
    pub fn Content(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Content)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContent)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetContentTemplate<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DataTemplate>,
    {
        let this = &windows_core::Interface::cast::<IContentControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContentTemplate)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn ITabViewItemFactory<R, F: FnOnce(&ITabViewItemFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TabViewItem, ITabViewItemFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn ITabViewItemStatics<R, F: FnOnce(&ITabViewItemStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TabViewItem, ITabViewItemStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for TabViewItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITabViewItem>();
}
unsafe impl windows_core::Interface for TabViewItem {
    type Vtable = <ITabViewItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITabViewItem as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TabViewItem {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TabViewItem";
}
unsafe impl Send for TabViewItem {}
unsafe impl Sync for TabViewItem {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabViewTabDragCompletedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TabViewTabDragCompletedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for TabViewTabDragCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITabViewTabDragCompletedEventArgs>();
}
unsafe impl windows_core::Interface for TabViewTabDragCompletedEventArgs {
    type Vtable = <ITabViewTabDragCompletedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ITabViewTabDragCompletedEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TabViewTabDragCompletedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TabViewTabDragCompletedEventArgs";
}
unsafe impl Send for TabViewTabDragCompletedEventArgs {}
unsafe impl Sync for TabViewTabDragCompletedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabViewTabDragStartingEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TabViewTabDragStartingEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl TabViewTabDragStartingEventArgs {
    pub fn Tab(&self) -> windows_core::Result<TabViewItem> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Tab)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for TabViewTabDragStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITabViewTabDragStartingEventArgs>();
}
unsafe impl windows_core::Interface for TabViewTabDragStartingEventArgs {
    type Vtable = <ITabViewTabDragStartingEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ITabViewTabDragStartingEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TabViewTabDragStartingEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TabViewTabDragStartingEventArgs";
}
unsafe impl Send for TabViewTabDragStartingEventArgs {}
unsafe impl Sync for TabViewTabDragStartingEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabViewTabDroppedOutsideEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TabViewTabDroppedOutsideEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl TabViewTabDroppedOutsideEventArgs {
    pub fn Tab(&self) -> windows_core::Result<TabViewItem> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Tab)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for TabViewTabDroppedOutsideEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITabViewTabDroppedOutsideEventArgs>();
}
unsafe impl windows_core::Interface for TabViewTabDroppedOutsideEventArgs {
    type Vtable = <ITabViewTabDroppedOutsideEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ITabViewTabDroppedOutsideEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TabViewTabDroppedOutsideEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TabViewTabDroppedOutsideEventArgs";
}
unsafe impl Send for TabViewTabDroppedOutsideEventArgs {}
unsafe impl Sync for TabViewTabDroppedOutsideEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextBlock(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TextBlock,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(TextBlock, FrameworkElement, UIElement, DependencyObject);
impl TextBlock {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            TextBlock,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetFontSize(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetFontSize)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetFontFamily)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Text(&self) -> windows_core::Result<windows_core::HSTRING> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Text)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| core::mem::transmute(result__))
        }
    }
    pub fn SetText(&self, value: &windows_core::HSTRING) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetText)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(value),
            )
            .ok()
        }
    }
    pub fn Inlines(&self) -> windows_core::Result<InlineCollection> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Inlines)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn ContentStart(&self) -> windows_core::Result<TextPointer> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ContentStart)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn TextHighlighters(
        &self,
    ) -> windows_core::Result<windows_collections::IVector<TextHighlighter>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).TextHighlighters)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn ITextBlockStatics<R, F: FnOnce(&ITextBlockStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TextBlock, ITextBlockStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for TextBlock {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITextBlock>();
}
unsafe impl windows_core::Interface for TextBlock {
    type Vtable = <ITextBlock as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITextBlock as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TextBlock {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TextBlock";
}
unsafe impl Send for TextBlock {}
unsafe impl Sync for TextBlock {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextBox(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TextBox,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    TextBox,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl TextBox {
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn GetRectFromCharacterIndex(
        &self,
        charindex: i32,
        trailingedge: bool,
    ) -> windows_core::Result<Rect> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetRectFromCharacterIndex)(
                windows_core::Interface::as_raw(self),
                charindex,
                trailingedge,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn ITextBoxFactory<R, F: FnOnce(&ITextBoxFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TextBox, ITextBoxFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn ITextBoxStatics<R, F: FnOnce(&ITextBoxStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TextBox, ITextBoxStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for TextBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITextBox>();
}
unsafe impl windows_core::Interface for TextBox {
    type Vtable = <ITextBox as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITextBox as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TextBox {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TextBox";
}
unsafe impl Send for TextBox {}
unsafe impl Sync for TextBox {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextElement(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TextElement,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(TextElement, DependencyObject);
impl TextElement {
    pub fn ContentStart(&self) -> windows_core::Result<TextPointer> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ContentStart)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    fn ITextElementStatics<R, F: FnOnce(&ITextElementStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TextElement, ITextElementStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for TextElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITextElement>();
}
unsafe impl windows_core::Interface for TextElement {
    type Vtable = <ITextElement as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITextElement as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TextElement {
    const NAME: &'static str = "Microsoft.UI.Xaml.Documents.TextElement";
}
unsafe impl Send for TextElement {}
unsafe impl Sync for TextElement {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextHighlighter(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TextHighlighter,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl TextHighlighter {
    pub fn Ranges(&self) -> windows_core::Result<windows_collections::IVector<TextRange>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Ranges)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetBackground<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Brush>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetBackground)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn new() -> windows_core::Result<Self> {
        Self::ITextHighlighterFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    pub fn compose<T>(compose: T) -> windows_core::Result<Self>
    where
        T: windows_core::Compose,
    {
        Self::ITextHighlighterFactory(|this| unsafe {
            let (derived__, base__) = windows_core::Compose::compose(compose);
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(&derived__),
                base__ as *mut _ as _,
                &mut result__,
            )
            .ok()?;
            let _ = &derived__;
            windows_core::imp::Type::from_abi(result__)
        })
    }
    fn ITextHighlighterFactory<
        R,
        F: FnOnce(&ITextHighlighterFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TextHighlighter, ITextHighlighterFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn ITextHighlighterStatics<
        R,
        F: FnOnce(&ITextHighlighterStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TextHighlighter, ITextHighlighterStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for TextHighlighter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITextHighlighter>();
}
unsafe impl windows_core::Interface for TextHighlighter {
    type Vtable = <ITextHighlighter as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITextHighlighter as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TextHighlighter {
    const NAME: &'static str = "Microsoft.UI.Xaml.Documents.TextHighlighter";
}
unsafe impl Send for TextHighlighter {}
unsafe impl Sync for TextHighlighter {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextPointer(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TextPointer,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl TextPointer {
    pub fn Offset(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Offset)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn GetCharacterRect(&self, direction: LogicalDirection) -> windows_core::Result<Rect> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetCharacterRect)(
                windows_core::Interface::as_raw(self),
                direction,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn GetPositionAtOffset(
        &self,
        offset: i32,
        direction: LogicalDirection,
    ) -> windows_core::Result<Self> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetPositionAtOffset)(
                windows_core::Interface::as_raw(self),
                offset,
                direction,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for TextPointer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITextPointer>();
}
unsafe impl windows_core::Interface for TextPointer {
    type Vtable = <ITextPointer as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITextPointer as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TextPointer {
    const NAME: &'static str = "Microsoft.UI.Xaml.Documents.TextPointer";
}
unsafe impl Send for TextPointer {}
unsafe impl Sync for TextPointer {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextRange {
    pub StartIndex: i32,
    pub Length: i32,
}
impl windows_core::imp::TypeKind for TextRange {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for TextRange {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"struct(Microsoft.UI.Xaml.Documents.TextRange;i4;i4)",
    );
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Documents.TextRange");
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Thickness {
    pub Left: f64,
    pub Top: f64,
    pub Right: f64,
    pub Bottom: f64,
}
impl windows_core::imp::TypeKind for Thickness {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for Thickness {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"struct(Microsoft.UI.Xaml.Thickness;f8;f8;f8;f8)",
    );
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Thickness");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeView(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TreeView,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    TreeView,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl TreeView {
    pub fn SetFontFamily<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FontFamily>,
    {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetFontFamily)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHorizontalContentAlignment(
        &self,
        value: HorizontalAlignment,
    ) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IControl>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalContentAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Resources)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResources<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ResourceDictionary>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetResources)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Tag(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Tag)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetTag<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetTag)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualWidth)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ActualHeight)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxWidth)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxHeight(&self, value: f64) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMaxHeight)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetHorizontalAlignment(&self, value: HorizontalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetHorizontalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVerticalAlignment(&self, value: VerticalAlignment) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVerticalAlignment)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetMargin(&self, value: Thickness) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetMargin)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DataContext(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).DataContext)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetDataContext<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetDataContext)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn RequestedTheme(&self) -> windows_core::Result<ElementTheme> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).RequestedTheme)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IFrameworkElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetRequestedTheme)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn DragItemsStarting<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<Self>, windows_core::Ref<TreeViewDragItemsStartingEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<ITreeView2>(self)?;
        let handler =
            <TypedEventHandler<Self, TreeViewDragItemsStartingEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragItemsStarting)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragItemsStarting,
            ))
        }
    }
    pub fn DragItemsCompleted<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<Self>, windows_core::Ref<TreeViewDragItemsCompletedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<ITreeView2>(self)?;
        let handler =
            <TypedEventHandler<Self, TreeViewDragItemsCompletedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragItemsCompleted)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragItemsCompleted,
            ))
        }
    }
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetAllowDrop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetVisibility)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContextFlyout)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetContextFlyout)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).XamlRoot)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetXamlRoot)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).IsTabStop)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetIsTabStop)(
                windows_core::Interface::as_raw(this),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).KeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DragOver)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).Drop)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerPressed)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerMoved)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerReleased)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PointerWheelChanged)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).DoubleTapped)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<UIElement>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler =
            <TypedEventHandler<UIElement, ContextRequestedEventArgs>>::new(move |a0, a1| {
                handler(a0, a1);
                Ok(())
            });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).ContextRequested)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(this).PreviewKeyDown)(
                windows_core::Interface::as_raw(this),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                this.clone(),
                token__,
                windows_core::Interface::vtable(this).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CapturePointer)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).ReleasePointerCapture)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoView)(
                windows_core::Interface::as_raw(this),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(this),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        let this = &windows_core::Interface::cast::<IUIElement>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Focus)(
                windows_core::Interface::as_raw(this),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn ITreeViewFactory<R, F: FnOnce(&ITreeViewFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TreeView, ITreeViewFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn ITreeViewStatics<R, F: FnOnce(&ITreeViewStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TreeView, ITreeViewStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn ITreeViewStatics2<R, F: FnOnce(&ITreeViewStatics2) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TreeView, ITreeViewStatics2> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for TreeView {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITreeView>();
}
unsafe impl windows_core::Interface for TreeView {
    type Vtable = <ITreeView as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITreeView as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TreeView {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TreeView";
}
unsafe impl Send for TreeView {}
unsafe impl Sync for TreeView {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeViewDragItemsCompletedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TreeViewDragItemsCompletedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl TreeViewDragItemsCompletedEventArgs {
    pub fn Items(
        &self,
    ) -> windows_core::Result<windows_collections::IVectorView<windows_core::IInspectable>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Items)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn NewParentItem(&self) -> windows_core::Result<windows_core::IInspectable> {
        let this = &windows_core::Interface::cast::<ITreeViewDragItemsCompletedEventArgs2>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).NewParentItem)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for TreeViewDragItemsCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITreeViewDragItemsCompletedEventArgs>();
}
unsafe impl windows_core::Interface for TreeViewDragItemsCompletedEventArgs {
    type Vtable = <ITreeViewDragItemsCompletedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ITreeViewDragItemsCompletedEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TreeViewDragItemsCompletedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TreeViewDragItemsCompletedEventArgs";
}
unsafe impl Send for TreeViewDragItemsCompletedEventArgs {}
unsafe impl Sync for TreeViewDragItemsCompletedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeViewDragItemsStartingEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TreeViewDragItemsStartingEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl TreeViewDragItemsStartingEventArgs {
    pub fn Cancel(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Cancel)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetCancel(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetCancel)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Items(
        &self,
    ) -> windows_core::Result<windows_collections::IVector<windows_core::IInspectable>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Items)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for TreeViewDragItemsStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITreeViewDragItemsStartingEventArgs>();
}
unsafe impl windows_core::Interface for TreeViewDragItemsStartingEventArgs {
    type Vtable = <ITreeViewDragItemsStartingEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ITreeViewDragItemsStartingEventArgs as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TreeViewDragItemsStartingEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TreeViewDragItemsStartingEventArgs";
}
unsafe impl Send for TreeViewDragItemsStartingEventArgs {}
unsafe impl Sync for TreeViewDragItemsStartingEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeViewNode(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TreeViewNode,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(TreeViewNode, DependencyObject);
impl TreeViewNode {
    pub fn Children(&self) -> windows_core::Result<windows_collections::IVector<Self>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Children)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    fn ITreeViewNodeFactory<R, F: FnOnce(&ITreeViewNodeFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TreeViewNode, ITreeViewNodeFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn ITreeViewNodeStatics<R, F: FnOnce(&ITreeViewNodeStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TreeViewNode, ITreeViewNodeStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for TreeViewNode {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITreeViewNode>();
}
unsafe impl windows_core::Interface for TreeViewNode {
    type Vtable = <ITreeViewNode as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITreeViewNode as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for TreeViewNode {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TreeViewNode";
}
unsafe impl Send for TreeViewNode {}
unsafe impl Sync for TreeViewNode {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedEventHandler<TSender, TResult>(
    windows_core::IUnknown,
    core::marker::PhantomData<TSender>,
    core::marker::PhantomData<TResult>,
)
where
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static;
unsafe impl<
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static,
> windows_core::Interface for TypedEventHandler<TSender, TResult>
{
    type Vtable = TypedEventHandler_Vtbl<TSender, TResult>;
    const IID: windows_core::GUID =
        windows_core::GUID::from_signature(<Self as windows_core::RuntimeType>::SIGNATURE);
}
impl<TSender: windows_core::RuntimeType + 'static, TResult: windows_core::RuntimeType + 'static>
    windows_core::RuntimeType for TypedEventHandler<TSender, TResult>
{
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::new()
        .push_slice(b"pinterface({9de1c534-6ae1-11e0-84e1-18a905bcc53f}")
        .push_slice(b";")
        .push_other(TSender::SIGNATURE)
        .push_slice(b";")
        .push_other(TResult::SIGNATURE)
        .push_slice(b")");
}
impl<TSender: windows_core::RuntimeType + 'static, TResult: windows_core::RuntimeType + 'static>
    TypedEventHandler<TSender, TResult>
{
    pub fn new<
        F: Fn(windows_core::Ref<TSender>, windows_core::Ref<TResult>) -> windows_core::Result<()>
            + Send
            + 'static,
    >(
        invoke: F,
    ) -> Self {
        let com = windows_core::imp::DelegateBox::<Self, F>::new(
            &TypedEventHandlerBox::<TSender, TResult, F>::VTABLE,
            invoke,
        );
        unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
    }
    pub fn Invoke<P0, P1>(&self, sender: P0, args: P1) -> windows_core::Result<()>
    where
        P0: windows_core::Param<TSender>,
        P1: windows_core::Param<TResult>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).Invoke)(
                windows_core::Interface::as_raw(self),
                sender.param().abi(),
                args.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct TypedEventHandler_Vtbl<TSender, TResult>
where
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static,
{
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: windows_core::imp::AbiType<TSender>,
        args: windows_core::imp::AbiType<TResult>,
    ) -> windows_core::HRESULT,
    TSender: core::marker::PhantomData<TSender>,
    TResult: core::marker::PhantomData<TResult>,
}
struct TypedEventHandlerBox<
    TSender,
    TResult,
    F: Fn(windows_core::Ref<TSender>, windows_core::Ref<TResult>) -> windows_core::Result<()>
        + Send
        + 'static,
>(core::marker::PhantomData<(TSender, TResult, fn() -> F)>)
where
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static;
impl<
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static,
    F: Fn(windows_core::Ref<TSender>, windows_core::Ref<TResult>) -> windows_core::Result<()>
        + Send
        + 'static,
> TypedEventHandlerBox<TSender, TResult, F>
{
    const VTABLE : TypedEventHandler_Vtbl < TSender , TResult , > = TypedEventHandler_Vtbl::< TSender , TResult , > { base__ : windows_core::IUnknown_Vtbl { QueryInterface : windows_core::imp::DelegateBox::< TypedEventHandler < TSender , TResult > , F >::QueryInterface , AddRef : windows_core::imp::DelegateBox::< TypedEventHandler < TSender , TResult > , F >::AddRef , Release : windows_core::imp::DelegateBox::< TypedEventHandler < TSender , TResult > , F >::Release , } , Invoke : Self::Invoke , TSender : core::marker::PhantomData::< TSender > , TResult : core::marker::PhantomData::< TResult > } ;
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: windows_core::imp::AbiType<TSender>,
        args: windows_core::imp::AbiType<TResult>,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<TypedEventHandler<TSender, TResult>, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&args),
            )
            .into()
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Typography(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Typography,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl Typography {
    pub fn SetVariants<P0>(element: P0, value: FontVariants) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DependencyObject>,
    {
        Self::ITypographyStatics(|this| unsafe {
            (windows_core::Interface::vtable(this).SetVariants)(
                windows_core::Interface::as_raw(this),
                element.param().abi(),
                value,
            )
            .ok()
        })
    }
    fn ITypographyStatics<R, F: FnOnce(&ITypographyStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Typography, ITypographyStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Typography {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITypography>();
}
unsafe impl windows_core::Interface for Typography {
    type Vtable = <ITypography as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITypography as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for Typography {
    const NAME: &'static str = "Microsoft.UI.Xaml.Documents.Typography";
}
unsafe impl Send for Typography {}
unsafe impl Sync for Typography {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UIElement(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    UIElement,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(UIElement, DependencyObject);
impl UIElement {
    pub fn SetAllowDrop(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetAllowDrop)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetVisibility)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ContextFlyout)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetContextFlyout)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).XamlRoot)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetXamlRoot)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsTabStop(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsTabStop)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsTabStop(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsTabStop)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).KeyDown)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveKeyDown,
            ))
        }
    }
    pub fn DragOver<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).DragOver)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveDragOver,
            ))
        }
    }
    pub fn Drop<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<DragEventArgs>)
            + Send
            + 'static,
    {
        let handler = <DragEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Drop)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveDrop,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).PointerPressed)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).PointerMoved)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).PointerReleased)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerWheelChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let handler = <PointerEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).PointerWheelChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemovePointerWheelChanged,
            ))
        }
    }
    pub fn DoubleTapped<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<DoubleTappedRoutedEventArgs>,
            ) + Send
            + 'static,
    {
        let handler = <DoubleTappedEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).DoubleTapped)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveDoubleTapped,
            ))
        }
    }
    pub fn ContextRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<Self>, windows_core::Ref<ContextRequestedEventArgs>)
            + Send
            + 'static,
    {
        let handler = <TypedEventHandler<Self, ContextRequestedEventArgs>>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).ContextRequested)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveContextRequested,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + Send
            + 'static,
    {
        let handler = <KeyEventHandler>::new(move |a0, a1| {
            handler(a0, a1);
            Ok(())
        });
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).PreviewKeyDown)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CapturePointer)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ReleasePointerCapture)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn StartBringIntoView(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).StartBringIntoView)(
                windows_core::Interface::as_raw(self),
            )
            .ok()
        }
    }
    pub fn StartBringIntoViewWithOptions<P0>(&self, options: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<BringIntoViewOptions>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).StartBringIntoViewWithOptions)(
                windows_core::Interface::as_raw(self),
                options.param().abi(),
            )
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Focus)(
                windows_core::Interface::as_raw(self),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        let this = &windows_core::Interface::cast::<IUIElementProtected>(self)?;
        unsafe {
            (windows_core::Interface::vtable(this).SetProtectedCursor)(
                windows_core::Interface::as_raw(this),
                value.param().abi(),
            )
            .ok()
        }
    }
    fn IUIElementStatics<R, F: FnOnce(&IUIElementStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<UIElement, IUIElementStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for UIElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IUIElement>();
}
unsafe impl windows_core::Interface for UIElement {
    type Vtable = <IUIElement as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IUIElement as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for UIElement {
    const NAME: &'static str = "Microsoft.UI.Xaml.UIElement";
}
unsafe impl Send for UIElement {}
unsafe impl Sync for UIElement {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VerticalAlignment(pub i32);
impl VerticalAlignment {
    pub const Top: Self = Self(0);
    pub const Center: Self = Self(1);
    pub const Bottom: Self = Self(2);
    pub const Stretch: Self = Self(3);
}
impl windows_core::imp::TypeKind for VerticalAlignment {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for VerticalAlignment {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Microsoft.UI.Xaml.VerticalAlignment;i4)");
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.VerticalAlignment");
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VirtualKey(pub i32);
impl VirtualKey {
    pub const None: Self = Self(0);
    pub const LeftButton: Self = Self(1);
    pub const RightButton: Self = Self(2);
    pub const Cancel: Self = Self(3);
    pub const MiddleButton: Self = Self(4);
    pub const XButton1: Self = Self(5);
    pub const XButton2: Self = Self(6);
    pub const Back: Self = Self(8);
    pub const Tab: Self = Self(9);
    pub const Clear: Self = Self(12);
    pub const Enter: Self = Self(13);
    pub const Shift: Self = Self(16);
    pub const Control: Self = Self(17);
    pub const Menu: Self = Self(18);
    pub const Pause: Self = Self(19);
    pub const CapitalLock: Self = Self(20);
    pub const Kana: Self = Self(21);
    pub const Hangul: Self = Self(21);
    pub const ImeOn: Self = Self(22);
    pub const Junja: Self = Self(23);
    pub const Final: Self = Self(24);
    pub const Hanja: Self = Self(25);
    pub const Kanji: Self = Self(25);
    pub const ImeOff: Self = Self(26);
    pub const Escape: Self = Self(27);
    pub const Convert: Self = Self(28);
    pub const NonConvert: Self = Self(29);
    pub const Accept: Self = Self(30);
    pub const ModeChange: Self = Self(31);
    pub const Space: Self = Self(32);
    pub const PageUp: Self = Self(33);
    pub const PageDown: Self = Self(34);
    pub const End: Self = Self(35);
    pub const Home: Self = Self(36);
    pub const Left: Self = Self(37);
    pub const Up: Self = Self(38);
    pub const Right: Self = Self(39);
    pub const Down: Self = Self(40);
    pub const Select: Self = Self(41);
    pub const Print: Self = Self(42);
    pub const Execute: Self = Self(43);
    pub const Snapshot: Self = Self(44);
    pub const Insert: Self = Self(45);
    pub const Delete: Self = Self(46);
    pub const Help: Self = Self(47);
    pub const Number0: Self = Self(48);
    pub const Number1: Self = Self(49);
    pub const Number2: Self = Self(50);
    pub const Number3: Self = Self(51);
    pub const Number4: Self = Self(52);
    pub const Number5: Self = Self(53);
    pub const Number6: Self = Self(54);
    pub const Number7: Self = Self(55);
    pub const Number8: Self = Self(56);
    pub const Number9: Self = Self(57);
    pub const A: Self = Self(65);
    pub const B: Self = Self(66);
    pub const C: Self = Self(67);
    pub const D: Self = Self(68);
    pub const E: Self = Self(69);
    pub const F: Self = Self(70);
    pub const G: Self = Self(71);
    pub const H: Self = Self(72);
    pub const I: Self = Self(73);
    pub const J: Self = Self(74);
    pub const K: Self = Self(75);
    pub const L: Self = Self(76);
    pub const M: Self = Self(77);
    pub const N: Self = Self(78);
    pub const O: Self = Self(79);
    pub const P: Self = Self(80);
    pub const Q: Self = Self(81);
    pub const R: Self = Self(82);
    pub const S: Self = Self(83);
    pub const T: Self = Self(84);
    pub const U: Self = Self(85);
    pub const V: Self = Self(86);
    pub const W: Self = Self(87);
    pub const X: Self = Self(88);
    pub const Y: Self = Self(89);
    pub const Z: Self = Self(90);
    pub const LeftWindows: Self = Self(91);
    pub const RightWindows: Self = Self(92);
    pub const Application: Self = Self(93);
    pub const Sleep: Self = Self(95);
    pub const NumberPad0: Self = Self(96);
    pub const NumberPad1: Self = Self(97);
    pub const NumberPad2: Self = Self(98);
    pub const NumberPad3: Self = Self(99);
    pub const NumberPad4: Self = Self(100);
    pub const NumberPad5: Self = Self(101);
    pub const NumberPad6: Self = Self(102);
    pub const NumberPad7: Self = Self(103);
    pub const NumberPad8: Self = Self(104);
    pub const NumberPad9: Self = Self(105);
    pub const Multiply: Self = Self(106);
    pub const Add: Self = Self(107);
    pub const Separator: Self = Self(108);
    pub const Subtract: Self = Self(109);
    pub const Decimal: Self = Self(110);
    pub const Divide: Self = Self(111);
    pub const F1: Self = Self(112);
    pub const F2: Self = Self(113);
    pub const F3: Self = Self(114);
    pub const F4: Self = Self(115);
    pub const F5: Self = Self(116);
    pub const F6: Self = Self(117);
    pub const F7: Self = Self(118);
    pub const F8: Self = Self(119);
    pub const F9: Self = Self(120);
    pub const F10: Self = Self(121);
    pub const F11: Self = Self(122);
    pub const F12: Self = Self(123);
    pub const F13: Self = Self(124);
    pub const F14: Self = Self(125);
    pub const F15: Self = Self(126);
    pub const F16: Self = Self(127);
    pub const F17: Self = Self(128);
    pub const F18: Self = Self(129);
    pub const F19: Self = Self(130);
    pub const F20: Self = Self(131);
    pub const F21: Self = Self(132);
    pub const F22: Self = Self(133);
    pub const F23: Self = Self(134);
    pub const F24: Self = Self(135);
    pub const NavigationView: Self = Self(136);
    pub const NavigationMenu: Self = Self(137);
    pub const NavigationUp: Self = Self(138);
    pub const NavigationDown: Self = Self(139);
    pub const NavigationLeft: Self = Self(140);
    pub const NavigationRight: Self = Self(141);
    pub const NavigationAccept: Self = Self(142);
    pub const NavigationCancel: Self = Self(143);
    pub const NumberKeyLock: Self = Self(144);
    pub const Scroll: Self = Self(145);
    pub const LeftShift: Self = Self(160);
    pub const RightShift: Self = Self(161);
    pub const LeftControl: Self = Self(162);
    pub const RightControl: Self = Self(163);
    pub const LeftMenu: Self = Self(164);
    pub const RightMenu: Self = Self(165);
    pub const GoBack: Self = Self(166);
    pub const GoForward: Self = Self(167);
    pub const Refresh: Self = Self(168);
    pub const Stop: Self = Self(169);
    pub const Search: Self = Self(170);
    pub const Favorites: Self = Self(171);
    pub const GoHome: Self = Self(172);
    pub const GamepadA: Self = Self(195);
    pub const GamepadB: Self = Self(196);
    pub const GamepadX: Self = Self(197);
    pub const GamepadY: Self = Self(198);
    pub const GamepadRightShoulder: Self = Self(199);
    pub const GamepadLeftShoulder: Self = Self(200);
    pub const GamepadLeftTrigger: Self = Self(201);
    pub const GamepadRightTrigger: Self = Self(202);
    pub const GamepadDPadUp: Self = Self(203);
    pub const GamepadDPadDown: Self = Self(204);
    pub const GamepadDPadLeft: Self = Self(205);
    pub const GamepadDPadRight: Self = Self(206);
    pub const GamepadMenu: Self = Self(207);
    pub const GamepadView: Self = Self(208);
    pub const GamepadLeftThumbstickButton: Self = Self(209);
    pub const GamepadRightThumbstickButton: Self = Self(210);
    pub const GamepadLeftThumbstickUp: Self = Self(211);
    pub const GamepadLeftThumbstickDown: Self = Self(212);
    pub const GamepadLeftThumbstickRight: Self = Self(213);
    pub const GamepadLeftThumbstickLeft: Self = Self(214);
    pub const GamepadRightThumbstickUp: Self = Self(215);
    pub const GamepadRightThumbstickDown: Self = Self(216);
    pub const GamepadRightThumbstickRight: Self = Self(217);
    pub const GamepadRightThumbstickLeft: Self = Self(218);
}
impl windows_core::imp::TypeKind for VirtualKey {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for VirtualKey {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Windows.System.VirtualKey;i4)");
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Windows.System.VirtualKey");
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VirtualKeyModifiers(pub u32);
impl VirtualKeyModifiers {
    pub const None: Self = Self(0);
    pub const Control: Self = Self(1);
    pub const Menu: Self = Self(2);
    pub const Shift: Self = Self(4);
    pub const Windows: Self = Self(8);
}
impl windows_core::imp::TypeKind for VirtualKeyModifiers {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for VirtualKeyModifiers {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Windows.System.VirtualKeyModifiers;u4)");
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Windows.System.VirtualKeyModifiers");
}
impl VirtualKeyModifiers {
    pub const fn contains(&self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}
impl core::ops::BitOr for VirtualKeyModifiers {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}
impl core::ops::BitAnd for VirtualKeyModifiers {
    type Output = Self;
    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}
impl core::ops::BitOrAssign for VirtualKeyModifiers {
    fn bitor_assign(&mut self, other: Self) {
        self.0.bitor_assign(other.0);
    }
}
impl core::ops::BitAndAssign for VirtualKeyModifiers {
    fn bitand_assign(&mut self, other: Self) {
        self.0.bitand_assign(other.0);
    }
}
impl core::ops::Not for VirtualKeyModifiers {
    type Output = Self;
    fn not(self) -> Self {
        Self(self.0.not())
    }
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Visibility(pub i32);
impl Visibility {
    pub const Visible: Self = Self(0);
    pub const Collapsed: Self = Self(1);
}
impl windows_core::imp::TypeKind for Visibility {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for Visibility {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Microsoft.UI.Xaml.Visibility;i4)");
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.Visibility");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VisualTreeHelper(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    VisualTreeHelper,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl VisualTreeHelper {
    pub fn GetChild<P0>(reference: P0, childindex: i32) -> windows_core::Result<DependencyObject>
    where
        P0: windows_core::Param<DependencyObject>,
    {
        Self::IVisualTreeHelperStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetChild)(
                windows_core::Interface::as_raw(this),
                reference.param().abi(),
                childindex,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    pub fn GetChildrenCount<P0>(reference: P0) -> windows_core::Result<i32>
    where
        P0: windows_core::Param<DependencyObject>,
    {
        Self::IVisualTreeHelperStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetChildrenCount)(
                windows_core::Interface::as_raw(this),
                reference.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        })
    }
    pub fn GetParent<P0>(reference: P0) -> windows_core::Result<DependencyObject>
    where
        P0: windows_core::Param<DependencyObject>,
    {
        Self::IVisualTreeHelperStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetParent)(
                windows_core::Interface::as_raw(this),
                reference.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IVisualTreeHelperStatics<
        R,
        F: FnOnce(&IVisualTreeHelperStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<VisualTreeHelper, IVisualTreeHelperStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for VisualTreeHelper {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IVisualTreeHelper>();
}
unsafe impl windows_core::Interface for VisualTreeHelper {
    type Vtable = <IVisualTreeHelper as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IVisualTreeHelper as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for VisualTreeHelper {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.VisualTreeHelper";
}
unsafe impl Send for VisualTreeHelper {}
unsafe impl Sync for VisualTreeHelper {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowId {
    pub Value: u64,
}
impl windows_core::imp::TypeKind for WindowId {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for WindowId {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"struct(Microsoft.UI.WindowId;u8)");
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.WindowId");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XamlReader(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    XamlReader,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl XamlReader {
    pub fn Load(xaml: &windows_core::HSTRING) -> windows_core::Result<windows_core::IInspectable> {
        Self::IXamlReaderStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Load)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(xaml),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IXamlReaderStatics<R, F: FnOnce(&IXamlReaderStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<XamlReader, IXamlReaderStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for XamlReader {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IXamlReader>();
}
unsafe impl windows_core::Interface for XamlReader {
    type Vtable = <IXamlReader as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IXamlReader as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for XamlReader {
    const NAME: &'static str = "Microsoft.UI.Xaml.Markup.XamlReader";
}
unsafe impl Send for XamlReader {}
unsafe impl Sync for XamlReader {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XamlRoot(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    XamlRoot,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl XamlRoot {
    pub fn ContentIslandEnvironment(&self) -> windows_core::Result<ContentIslandEnvironment> {
        let this = &windows_core::Interface::cast::<IXamlRoot2>(self)?;
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).ContentIslandEnvironment)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
impl windows_core::RuntimeType for XamlRoot {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IXamlRoot>();
}
unsafe impl windows_core::Interface for XamlRoot {
    type Vtable = <IXamlRoot as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IXamlRoot as windows_core::Interface>::IID;
}
impl windows_core::RuntimeName for XamlRoot {
    const NAME: &'static str = "Microsoft.UI.Xaml.XamlRoot";
}
unsafe impl Send for XamlRoot {}
unsafe impl Sync for XamlRoot {}
